use std::collections::HashMap;

use crate::config::SimulationConfig;
use crate::domain::{
    ClosedSimulationTrade, OpenSimulationPosition, SimulationReport, TradeSide, WalletActivity,
    WalletActivityType,
};

#[derive(Debug, Clone)]
struct PositionState {
    asset: String,
    title: Option<String>,
    leader_size: f64,
    follower_size: f64,
    avg_entry_price: f64,
    last_mark_price: f64,
    last_buy_timestamp: i64,
}

pub fn simulate_copy_trading(
    wallet: &str,
    activities: &[WalletActivity],
    current_marks: &HashMap<String, f64>,
    config: &SimulationConfig,
) -> SimulationReport {
    let mut cash = config.starting_cash;
    let mut followed_trades = 0usize;
    let mut ignored_trades = 0usize;
    let mut realized_pnl = 0.0;
    let mut open_positions: HashMap<String, PositionState> = HashMap::new();
    let mut closed_positions = Vec::new();

    let mut trade_activities = activities
        .iter()
        .filter(|activity| matches!(activity.activity_type, WalletActivityType::Trade))
        .filter(|activity| {
            matches!(activity.side, Some(TradeSide::Buy | TradeSide::Sell))
                && activity.price > 0.0
                && activity.size > 0.0
        })
        .collect::<Vec<_>>();
    trade_activities.sort_by_key(|activity| activity.timestamp);

    let tracked_from_timestamp = trade_activities.first().map(|activity| activity.timestamp);
    let tracked_to_timestamp = trade_activities.last().map(|activity| activity.timestamp);

    for activity in trade_activities {
        let side = activity.side.expect("filtered above");
        let price = bounded_price(activity.price);
        let simulated_timestamp = activity.timestamp + config.follow_delay_secs as i64;

        match side {
            TradeSide::Buy => {
                let target_usdc = (activity.usdc_size * config.wallet_scale)
                    .min(config.max_trade_usdc)
                    .min(cash);

                if target_usdc < config.minimum_trade_usdc {
                    ignored_trades += 1;
                    continue;
                }

                let entry_price = bounded_price(apply_buy_slippage(price, config.slippage_bps));
                let follower_size = target_usdc / entry_price;
                let position = open_positions
                    .entry(activity.asset.clone())
                    .or_insert_with(|| PositionState {
                        asset: activity.asset.clone(),
                        title: activity.title.clone(),
                        leader_size: 0.0,
                        follower_size: 0.0,
                        avg_entry_price: entry_price,
                        last_mark_price: entry_price,
                        last_buy_timestamp: simulated_timestamp,
                    });

                let total_size = position.follower_size + follower_size;
                let total_cost = (position.avg_entry_price * position.follower_size)
                    + (entry_price * follower_size);

                position.avg_entry_price = total_cost / total_size.max(f64::EPSILON);
                position.follower_size = total_size;
                position.leader_size += activity.size;
                position.last_mark_price = current_marks
                    .get(&activity.asset)
                    .copied()
                    .unwrap_or(entry_price);
                position.last_buy_timestamp = simulated_timestamp;

                cash -= target_usdc;
                followed_trades += 1;
            }
            TradeSide::Sell => {
                let Some(position) = open_positions.get_mut(&activity.asset) else {
                    ignored_trades += 1;
                    continue;
                };

                if position.leader_size <= 0.0 || position.follower_size <= 0.0 {
                    ignored_trades += 1;
                    continue;
                }

                let leader_size_before = position.leader_size;
                let sold_leader_size = activity.size.min(leader_size_before);
                let sell_ratio = (sold_leader_size / leader_size_before).clamp(0.0, 1.0);
                let sold_follower_size = position.follower_size * sell_ratio;

                if sold_follower_size <= 0.0 {
                    ignored_trades += 1;
                    continue;
                }

                let exit_price = bounded_price(apply_sell_slippage(price, config.slippage_bps));
                let proceeds = sold_follower_size * exit_price;
                let cost_basis = sold_follower_size * position.avg_entry_price;
                let trade_pnl = proceeds - cost_basis;

                position.follower_size -= sold_follower_size;
                position.leader_size = (position.leader_size - sold_leader_size).max(0.0);
                position.last_mark_price = current_marks
                    .get(&activity.asset)
                    .copied()
                    .unwrap_or(exit_price);

                cash += proceeds;
                realized_pnl += trade_pnl;
                followed_trades += 1;

                closed_positions.push(ClosedSimulationTrade {
                    asset: activity.asset.clone(),
                    title: activity.title.clone(),
                    buy_timestamp: position.last_buy_timestamp,
                    sell_timestamp: simulated_timestamp,
                    entry_price: position.avg_entry_price,
                    exit_price,
                    size: sold_follower_size,
                    pnl: trade_pnl,
                });
            }
            TradeSide::Unknown => {
                ignored_trades += 1;
            }
        }

        open_positions.retain(|_, position| position.follower_size > 0.000_001);
    }

    let open_positions = open_positions
        .into_values()
        .map(|position| {
            let mark_price = current_marks
                .get(&position.asset)
                .copied()
                .unwrap_or(position.last_mark_price);
            let unrealized_pnl = (mark_price - position.avg_entry_price) * position.follower_size;

            OpenSimulationPosition {
                asset: position.asset,
                title: position.title,
                size: position.follower_size,
                avg_entry_price: position.avg_entry_price,
                mark_price,
                unrealized_pnl,
            }
        })
        .collect::<Vec<_>>();

    let unrealized_pnl = open_positions
        .iter()
        .map(|position| position.unrealized_pnl)
        .sum();
    let final_equity = cash
        + open_positions
            .iter()
            .map(|position| position.size * position.mark_price)
            .sum::<f64>();
    let closed_trades = closed_positions.len();
    let wins = closed_positions
        .iter()
        .filter(|trade| trade.pnl > 0.0)
        .count();
    let win_rate = if closed_trades == 0 {
        0.0
    } else {
        wins as f64 / closed_trades as f64
    };

    SimulationReport {
        wallet: wallet.to_owned(),
        followed_trades,
        ignored_trades,
        closed_trades,
        win_rate,
        realized_pnl,
        unrealized_pnl,
        total_pnl: realized_pnl + unrealized_pnl,
        final_cash: cash,
        final_equity,
        starting_cash: config.starting_cash,
        tracked_from_timestamp,
        tracked_to_timestamp,
        open_positions,
        closed_positions,
    }
}

fn bounded_price(price: f64) -> f64 {
    price.clamp(0.001, 0.999)
}

fn apply_buy_slippage(price: f64, slippage_bps: f64) -> f64 {
    price * (1.0 + slippage_bps / 10_000.0)
}

fn apply_sell_slippage(price: f64, slippage_bps: f64) -> f64 {
    price * (1.0 - slippage_bps / 10_000.0)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::config::SimulationConfig;
    use crate::domain::{TradeSide, WalletActivity, WalletActivityType};

    use super::simulate_copy_trading;

    #[test]
    fn profitable_round_trip_produces_positive_pnl() {
        let activities = vec![
            WalletActivity {
                proxy_wallet: "0x1111111111111111111111111111111111111111".to_owned(),
                timestamp: 1_700_000_000,
                condition_id: "condition-1".to_owned(),
                activity_type: WalletActivityType::Trade,
                size: 10.0,
                usdc_size: 4.0,
                transaction_hash: None,
                price: 0.4,
                asset: "asset-1".to_owned(),
                side: Some(TradeSide::Buy),
                outcome_index: None,
                title: Some("Will rust win?".to_owned()),
                slug: None,
                event_slug: None,
                outcome: None,
                name: None,
                pseudonym: None,
            },
            WalletActivity {
                proxy_wallet: "0x1111111111111111111111111111111111111111".to_owned(),
                timestamp: 1_700_000_100,
                condition_id: "condition-1".to_owned(),
                activity_type: WalletActivityType::Trade,
                size: 10.0,
                usdc_size: 7.0,
                transaction_hash: None,
                price: 0.7,
                asset: "asset-1".to_owned(),
                side: Some(TradeSide::Sell),
                outcome_index: None,
                title: Some("Will rust win?".to_owned()),
                slug: None,
                event_slug: None,
                outcome: None,
                name: None,
                pseudonym: None,
            },
        ];

        let config = SimulationConfig {
            wallet_scale: 2.0,
            minimum_trade_usdc: 1.0,
            max_trade_usdc: 50.0,
            slippage_bps: 0.0,
            starting_cash: 100.0,
            ..SimulationConfig::default()
        };

        let report = simulate_copy_trading(
            "0x1111111111111111111111111111111111111111",
            &activities,
            &HashMap::new(),
            &config,
        );

        assert!(
            report.realized_pnl > 0.0,
            "report: {:?}",
            report.realized_pnl
        );
        assert_eq!(report.closed_trades, 1);
    }
}
