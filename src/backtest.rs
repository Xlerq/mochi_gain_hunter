use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use serde::Serialize;

use crate::config::{AppConfig, SimulationConfig};
use crate::polymarket::PolymarketClient;
use crate::reporting::build_wallet_analysis;
use crate::simulation::simulate_copy_trading;
use crate::storage::load_activity_log;

#[derive(Debug, Serialize)]
struct BacktestResultRow {
    follow_delay_secs: u64,
    slippage_bps: f64,
    wallet_scale: f64,
    followed_trades: usize,
    closed_trades: usize,
    win_rate: f64,
    realized_pnl: f64,
    unrealized_pnl: f64,
    total_pnl: f64,
    final_equity: f64,
}

#[derive(Debug, Serialize)]
struct BacktestOutput {
    wallet: String,
    label: Option<String>,
    used_stored_activity: bool,
    activity_count: usize,
    bankroll: f64,
    top_results: Vec<BacktestResultRow>,
}

pub async fn run_backtest(
    config_path: &Path,
    wallet_input: &str,
    top: Option<usize>,
    stored_only: bool,
) -> Result<()> {
    let config = AppConfig::load_or_default(config_path)?;
    let client = PolymarketClient::new(&config)?;
    let resolved = client.resolve_wallet_input(wallet_input).await?;
    let analysis = build_wallet_analysis(&client, &config, &resolved.wallet, None).await?;
    let stored_activities = load_activity_log(&config, &resolved.wallet)?;

    let used_stored_activity = stored_only && !stored_activities.is_empty();
    let activities = if used_stored_activity {
        stored_activities
    } else {
        merge_activities(stored_activities, analysis.activities.clone())
    };

    let mut rows = Vec::new();
    for follow_delay_secs in &config.backtest.delay_grid_secs {
        for slippage_bps in &config.backtest.slippage_grid_bps {
            for wallet_scale in &config.backtest.wallet_scale_grid {
                let simulation_config = SimulationConfig {
                    follow_delay_secs: *follow_delay_secs,
                    slippage_bps: *slippage_bps,
                    wallet_scale: *wallet_scale,
                    ..config.simulation.clone()
                };

                let report = simulate_copy_trading(
                    &resolved.wallet,
                    &activities,
                    &analysis.current_marks,
                    &simulation_config,
                );

                rows.push(BacktestResultRow {
                    follow_delay_secs: *follow_delay_secs,
                    slippage_bps: *slippage_bps,
                    wallet_scale: *wallet_scale,
                    followed_trades: report.followed_trades,
                    closed_trades: report.closed_trades,
                    win_rate: report.win_rate,
                    realized_pnl: report.realized_pnl,
                    unrealized_pnl: report.unrealized_pnl,
                    total_pnl: report.total_pnl,
                    final_equity: report.final_equity,
                });
            }
        }
    }

    rows.sort_by(|left, right| {
        right
            .total_pnl
            .partial_cmp(&left.total_pnl)
            .unwrap_or(Ordering::Equal)
    });
    rows.truncate(top.unwrap_or(config.backtest.max_results));

    let output = BacktestOutput {
        wallet: resolved.wallet,
        label: resolved.label.or(resolved.username),
        used_stored_activity,
        activity_count: activities.len(),
        bankroll: config.simulation.starting_cash,
        top_results: rows,
    };

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn merge_activities(
    mut stored_activities: Vec<crate::domain::WalletActivity>,
    live_activities: Vec<crate::domain::WalletActivity>,
) -> Vec<crate::domain::WalletActivity> {
    let mut seen = HashMap::new();
    for activity in stored_activities.drain(..).chain(live_activities) {
        let key = format!(
            "{}:{}:{}:{:.6}:{:.6}",
            activity.asset,
            activity.timestamp,
            activity.condition_id,
            activity.price,
            activity.usdc_size
        );
        seen.entry(key).or_insert(activity);
    }

    let mut activities = seen.into_values().collect::<Vec<_>>();
    activities.sort_by_key(|activity| activity.timestamp);
    activities
}
