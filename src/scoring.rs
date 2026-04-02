use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::ScoringConfig;
use crate::domain::{
    ClosedPosition, FollowRecommendation, LeaderboardEntry, Position, ScoreComponents,
    WalletActivity, WalletActivityType, WalletAggregates, WalletScorecard,
};

pub fn score_wallet(
    leaderboard_entry: Option<&LeaderboardEntry>,
    activities: &[WalletActivity],
    positions: &[Position],
    closed_positions: &[ClosedPosition],
    config: &ScoringConfig,
) -> WalletScorecard {
    let trade_activities = activities
        .iter()
        .filter(|activity| matches!(activity.activity_type, WalletActivityType::Trade))
        .collect::<Vec<_>>();

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default();

    let trade_count = trade_activities.len();
    let recent_trade_count = trade_activities
        .iter()
        .filter(|activity| now.saturating_sub(activity.timestamp) <= 30 * 24 * 60 * 60)
        .count();
    let average_trade_usdc = average(
        &trade_activities
            .iter()
            .map(|activity| activity.usdc_size)
            .collect::<Vec<_>>(),
    );
    let realized_pnl_total = closed_positions
        .iter()
        .map(|position| position.realized_pnl)
        .sum();
    let open_pnl_total = positions.iter().map(|position| position.cash_pnl).sum();
    let total_open_value = positions
        .iter()
        .map(|position| position.current_value.abs())
        .sum();
    let top_position_ratio = if total_open_value <= 0.0 {
        0.0
    } else {
        positions
            .iter()
            .map(|position| position.current_value.abs() / total_open_value)
            .fold(0.0, f64::max)
    };
    let positive_closed = closed_positions
        .iter()
        .filter(|position| position.realized_pnl > 0.0)
        .count();
    let win_rate = if closed_positions.is_empty() {
        0.0
    } else {
        positive_closed as f64 / closed_positions.len() as f64
    };
    let last_trade_timestamp = trade_activities
        .iter()
        .map(|activity| activity.timestamp)
        .max();
    let days_since_last_trade = last_trade_timestamp
        .map(|timestamp| now.saturating_sub(timestamp) as f64 / 86_400.0)
        .unwrap_or(config.stale_after_days as f64 * 2.0);

    let leaderboard_pnl = leaderboard_entry.map(|entry| entry.pnl).unwrap_or_default();
    let leaderboard_volume = leaderboard_entry
        .map(|entry| entry.volume)
        .unwrap_or_default();
    let leaderboard_rank = leaderboard_entry.map(|entry| entry.rank.clone());
    let user_name = leaderboard_entry
        .and_then(|entry| entry.user_name.clone())
        .or_else(|| {
            trade_activities
                .iter()
                .find_map(|activity| activity.pseudonym.clone())
        })
        .or_else(|| {
            trade_activities
                .iter()
                .find_map(|activity| activity.name.clone())
        });

    let components = ScoreComponents {
        realized_pnl_score: centered_pnl_score(realized_pnl_total, config.pnl_scale),
        open_pnl_score: centered_pnl_score(open_pnl_total, config.pnl_scale),
        consistency_score: clamp01(win_rate),
        activity_score: clamp01(trade_count as f64 / config.target_trade_count as f64),
        freshness_score: clamp01(1.0 - (days_since_last_trade / config.stale_after_days as f64)),
        leaderboard_score: leaderboard_score(leaderboard_entry, config.pnl_scale),
        concentration_penalty: clamp01(
            (top_position_ratio - config.max_position_concentration)
                / (1.0 - config.max_position_concentration),
        ),
    };

    let positive_weight_sum = config.weight_realized_pnl
        + config.weight_open_pnl
        + config.weight_consistency
        + config.weight_activity
        + config.weight_freshness
        + config.weight_leaderboard;

    let weighted_score = (components.realized_pnl_score * config.weight_realized_pnl)
        + (components.open_pnl_score * config.weight_open_pnl)
        + (components.consistency_score * config.weight_consistency)
        + (components.activity_score * config.weight_activity)
        + (components.freshness_score * config.weight_freshness)
        + (components.leaderboard_score * config.weight_leaderboard)
        - (components.concentration_penalty * config.weight_concentration_penalty);

    let score = (clamp01(weighted_score / positive_weight_sum) * 100.0 * 100.0).round() / 100.0;

    let mut gating_reasons = Vec::new();
    if trade_count < config.min_wallet_trades {
        gating_reasons.push(format!(
            "too few trades: {} < {}",
            trade_count, config.min_wallet_trades
        ));
    }
    if closed_positions.len() < config.min_closed_positions {
        gating_reasons.push(format!(
            "too few closed positions: {} < {}",
            closed_positions.len(),
            config.min_closed_positions
        ));
    }
    if realized_pnl_total < config.min_realized_pnl {
        gating_reasons.push(format!(
            "realized pnl below threshold: {:.2} < {:.2}",
            realized_pnl_total, config.min_realized_pnl
        ));
    }
    if win_rate < config.min_win_rate {
        gating_reasons.push(format!(
            "win rate below threshold: {:.2} < {:.2}",
            win_rate, config.min_win_rate
        ));
    }
    if top_position_ratio > config.max_position_concentration {
        gating_reasons.push(format!(
            "portfolio too concentrated: {:.2} > {:.2}",
            top_position_ratio, config.max_position_concentration
        ));
    }

    let eligible = gating_reasons.is_empty() && score >= config.min_follow_score;
    let recommendation = if eligible {
        FollowRecommendation::PaperFollow
    } else if score >= config.min_follow_score {
        FollowRecommendation::ManualReview
    } else if score >= 55.0 {
        FollowRecommendation::Watch
    } else {
        FollowRecommendation::Ignore
    };

    WalletScorecard {
        wallet: leaderboard_entry
            .map(|entry| entry.proxy_wallet.clone())
            .or_else(|| {
                trade_activities
                    .first()
                    .map(|activity| activity.proxy_wallet.clone())
            })
            .or_else(|| {
                positions
                    .first()
                    .map(|position| position.proxy_wallet.clone())
            })
            .or_else(|| {
                closed_positions
                    .first()
                    .map(|position| position.proxy_wallet.clone())
            })
            .unwrap_or_default(),
        user_name,
        leaderboard_rank,
        score,
        eligible,
        recommendation,
        gating_reasons,
        aggregates: WalletAggregates {
            trade_count,
            recent_trade_count,
            open_position_count: positions.len(),
            closed_position_count: closed_positions.len(),
            average_trade_usdc,
            realized_pnl_total,
            open_pnl_total,
            leaderboard_pnl,
            leaderboard_volume,
            total_open_value,
            top_position_ratio,
            win_rate,
            last_trade_timestamp,
        },
        components,
    }
}

fn average(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

fn clamp01(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}

fn centered_pnl_score(value: f64, scale: f64) -> f64 {
    0.5 + 0.5 * (value / scale.max(1.0)).tanh()
}

fn leaderboard_score(leaderboard_entry: Option<&LeaderboardEntry>, pnl_scale: f64) -> f64 {
    let Some(entry) = leaderboard_entry else {
        return 0.5;
    };

    let pnl_component = centered_pnl_score(entry.pnl, pnl_scale);
    let volume_component = clamp01(entry.volume / (pnl_scale * 20.0));
    (pnl_component * 0.7) + (volume_component * 0.3)
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::config::ScoringConfig;
    use crate::domain::{
        ClosedPosition, LeaderboardEntry, Position, WalletActivity, WalletActivityType,
        WalletScorecard,
    };

    use super::score_wallet;

    #[test]
    fn scores_stronger_wallets_higher() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or(1_700_000_000);
        let entry = LeaderboardEntry {
            rank: "1".to_owned(),
            proxy_wallet: "0x1111111111111111111111111111111111111111".to_owned(),
            user_name: Some("wallet_one".to_owned()),
            volume: 20_000.0,
            pnl: 4_000.0,
            profile_image: None,
            x_username: None,
            verified_badge: true,
        };
        let activities = vec![
            trade_activity(now - 300, 200.0),
            trade_activity(now - 240, 150.0),
            trade_activity(now - 180, 175.0),
            trade_activity(now - 120, 300.0),
            trade_activity(now - 60, 225.0),
        ];
        let positions = vec![
            Position {
                proxy_wallet: entry.proxy_wallet.clone(),
                asset: "asset-1".to_owned(),
                condition_id: "condition-1".to_owned(),
                size: 100.0,
                avg_price: 0.4,
                initial_value: 40.0,
                current_value: 75.0,
                cash_pnl: 35.0,
                percent_pnl: 0.0,
                total_bought: 40.0,
                realized_pnl: 0.0,
                percent_realized_pnl: 0.0,
                cur_price: 0.75,
                redeemable: false,
                mergeable: false,
                title: None,
                slug: None,
                event_slug: None,
                outcome: None,
                outcome_index: None,
                opposite_outcome: None,
                opposite_asset: None,
                end_date: None,
                negative_risk: false,
            },
            Position {
                proxy_wallet: entry.proxy_wallet.clone(),
                asset: "asset-2".to_owned(),
                condition_id: "condition-2".to_owned(),
                size: 80.0,
                avg_price: 0.35,
                initial_value: 28.0,
                current_value: 50.0,
                cash_pnl: 22.0,
                percent_pnl: 0.0,
                total_bought: 28.0,
                realized_pnl: 0.0,
                percent_realized_pnl: 0.0,
                cur_price: 0.625,
                redeemable: false,
                mergeable: false,
                title: None,
                slug: None,
                event_slug: None,
                outcome: None,
                outcome_index: None,
                opposite_outcome: None,
                opposite_asset: None,
                end_date: None,
                negative_risk: false,
            },
        ];
        let closed_positions = vec![
            closed_position(&entry.proxy_wallet, 180.0),
            closed_position(&entry.proxy_wallet, 50.0),
            closed_position(&entry.proxy_wallet, -10.0),
            closed_position(&entry.proxy_wallet, 90.0),
            closed_position(&entry.proxy_wallet, 40.0),
            closed_position(&entry.proxy_wallet, 30.0),
            closed_position(&entry.proxy_wallet, -5.0),
            closed_position(&entry.proxy_wallet, 25.0),
        ];

        let config = ScoringConfig {
            min_wallet_trades: 4,
            min_closed_positions: 5,
            min_follow_score: 60.0,
            target_trade_count: 5,
            pnl_scale: 250.0,
            ..ScoringConfig::default()
        };

        let scorecard: WalletScorecard = score_wallet(
            Some(&entry),
            &activities,
            &positions,
            &closed_positions,
            &config,
        );

        assert!(scorecard.score > 60.0, "score was {}", scorecard.score);
        assert!(scorecard.eligible);
    }

    fn trade_activity(timestamp: i64, usdc_size: f64) -> WalletActivity {
        WalletActivity {
            proxy_wallet: "0x1111111111111111111111111111111111111111".to_owned(),
            timestamp,
            condition_id: "condition-1".to_owned(),
            activity_type: WalletActivityType::Trade,
            size: 10.0,
            usdc_size,
            transaction_hash: None,
            price: 0.4,
            asset: "asset-1".to_owned(),
            side: None,
            outcome_index: None,
            title: None,
            slug: None,
            event_slug: None,
            outcome: None,
            name: None,
            pseudonym: None,
        }
    }

    fn closed_position(wallet: &str, realized_pnl: f64) -> ClosedPosition {
        ClosedPosition {
            proxy_wallet: wallet.to_owned(),
            asset: "asset-1".to_owned(),
            condition_id: "condition-1".to_owned(),
            avg_price: 0.3,
            total_bought: 100.0,
            realized_pnl,
            cur_price: 0.6,
            timestamp: 1_700_000_000,
            title: None,
            slug: None,
            event_slug: None,
            outcome: None,
            outcome_index: None,
            opposite_outcome: None,
            opposite_asset: None,
            end_date: None,
        }
    }
}
