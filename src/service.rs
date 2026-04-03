use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::Serialize;
use tokio::time::sleep;

use crate::config::{AlertConfig, AppConfig};
use crate::domain::{
    PortfolioSimulationExecution, PortfolioSimulationReport, SimulationExecutionStatus,
};
use crate::paper_runtime::{
    PaperRuntimeProgress, PaperRuntimeWalletInput, build_shared_paper_runtime,
};
use crate::polymarket::PolymarketClient;
use crate::reporting::{WalletAnalysis, build_wallet_analysis};
use crate::storage::persist_wallet_tracking;

const DESKTOP_ALERT_LIMIT_PER_CYCLE: usize = 5;

#[derive(Debug, Clone)]
struct ServiceWalletRow {
    config_index: usize,
    wallet: String,
    label: String,
    paper_follow_enabled: bool,
    analysis: WalletAnalysis,
}

#[derive(Debug)]
struct ServiceCycleOutcome {
    rows: Vec<ServiceWalletRow>,
    paper: PaperRuntimeProgress,
    stale_wallets: usize,
    dropped_wallets: usize,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ServiceStatusRecord {
    captured_at: i64,
    started_at: i64,
    cycle: usize,
    result: String,
    watchlist_wallets: usize,
    tracked_wallets: usize,
    stale_wallets: usize,
    dropped_wallets: usize,
    processed_activity_count: usize,
    resumed_journal: bool,
    replayed_history: bool,
    new_execution_count: usize,
    emitted_alert_count: usize,
    open_positions: usize,
    final_cash: f64,
    final_equity: f64,
    total_pnl: f64,
    warnings: Vec<String>,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
struct ServiceAlertRecord {
    captured_at: i64,
    cycle: usize,
    category: String,
    severity: String,
    message: String,
    source_wallet: String,
    source_label: Option<String>,
    asset: String,
    title: Option<String>,
    side: String,
    status: String,
    requested_usdc: f64,
    filled_usdc: f64,
    price: f64,
    reason: Option<String>,
    account_cash: f64,
    account_equity: f64,
}

pub async fn run_service(config_path: &Path, once: bool, cycles: Option<usize>) -> Result<()> {
    let config = AppConfig::load_or_default(config_path)?;
    let client = PolymarketClient::new(&config)?;
    let started_at = now_ts();
    let max_cycles = cycles.or_else(|| once.then_some(1));
    let mut cycle = 0usize;
    let mut previous_rows = HashMap::new();

    loop {
        cycle += 1;
        let cycle_result = run_service_cycle(&client, &config, &previous_rows).await;

        match cycle_result {
            Ok(outcome) => {
                let suppress_replay_alerts =
                    config.service.suppress_replay_alerts && outcome.paper.replayed_history;
                let alerts = if suppress_replay_alerts {
                    Vec::new()
                } else {
                    build_service_alerts(
                        cycle,
                        &config.alerts,
                        &outcome.paper.summary,
                        &outcome.paper.new_executions,
                    )
                };

                emit_alerts(&config.alerts, &alerts)?;
                persist_service_alerts(&config, &alerts)?;

                let status = ServiceStatusRecord {
                    captured_at: now_ts(),
                    started_at,
                    cycle,
                    result: "OK".to_owned(),
                    watchlist_wallets: config.monitor.wallets.len(),
                    tracked_wallets: outcome.rows.len(),
                    stale_wallets: outcome.stale_wallets,
                    dropped_wallets: outcome.dropped_wallets,
                    processed_activity_count: outcome.paper.processed_activity_count,
                    resumed_journal: outcome.paper.resumed_journal,
                    replayed_history: outcome.paper.replayed_history,
                    new_execution_count: outcome.paper.new_executions.len(),
                    emitted_alert_count: alerts.len(),
                    open_positions: outcome.paper.summary.open_positions.len(),
                    final_cash: outcome.paper.summary.final_cash,
                    final_equity: outcome.paper.summary.final_equity,
                    total_pnl: outcome.paper.summary.total_pnl,
                    warnings: outcome.warnings.clone(),
                    message: format!(
                        "service ok | journal {} | processed {} | alerts {} | stale {} | dropped {}",
                        if outcome.paper.resumed_journal {
                            "resumed"
                        } else {
                            "rebuilt"
                        },
                        outcome.paper.processed_activity_count,
                        alerts.len(),
                        outcome.stale_wallets,
                        outcome.dropped_wallets
                    ),
                };
                persist_service_status(&config, &status)?;

                if config.service.print_heartbeat {
                    println!(
                        "[service] cycle={} wallets={} processed={} alerts={} equity={:.2} cash={:.2} stale={} dropped={} journal={} pnl={:.2}",
                        cycle,
                        outcome.rows.len(),
                        outcome.paper.processed_activity_count,
                        alerts.len(),
                        outcome.paper.summary.final_equity,
                        outcome.paper.summary.final_cash,
                        outcome.stale_wallets,
                        outcome.dropped_wallets,
                        if outcome.paper.resumed_journal {
                            "resumed"
                        } else {
                            "rebuilt"
                        },
                        outcome.paper.summary.total_pnl
                    );
                }

                previous_rows = outcome
                    .rows
                    .into_iter()
                    .map(|row| (row.config_index, row))
                    .collect::<HashMap<_, _>>();
            }
            Err(error) => {
                let status = ServiceStatusRecord {
                    captured_at: now_ts(),
                    started_at,
                    cycle,
                    result: "ERROR".to_owned(),
                    watchlist_wallets: config.monitor.wallets.len(),
                    tracked_wallets: previous_rows.len(),
                    stale_wallets: 0,
                    dropped_wallets: config
                        .monitor
                        .wallets
                        .len()
                        .saturating_sub(previous_rows.len()),
                    processed_activity_count: 0,
                    resumed_journal: false,
                    replayed_history: false,
                    new_execution_count: 0,
                    emitted_alert_count: 0,
                    open_positions: 0,
                    final_cash: 0.0,
                    final_equity: 0.0,
                    total_pnl: 0.0,
                    warnings: vec![error.to_string()],
                    message: format_status_error("service cycle failed", &error),
                };
                persist_service_status(&config, &status)?;
                eprintln!("{}", status.message);

                if max_cycles == Some(1) {
                    return Err(error);
                }
            }
        }

        if max_cycles.is_some_and(|max_cycles| cycle >= max_cycles) {
            break;
        }

        sleep(Duration::from_secs(config.service.poll_interval_secs)).await;
    }

    Ok(())
}

async fn run_service_cycle(
    client: &PolymarketClient,
    config: &AppConfig,
    previous_rows: &HashMap<usize, ServiceWalletRow>,
) -> Result<ServiceCycleOutcome> {
    let mut rows = Vec::new();
    let mut warnings = Vec::new();
    let mut stale_wallets = 0usize;
    let mut dropped_wallets = 0usize;

    for (config_index, watched) in config.monitor.wallets.iter().enumerate() {
        let resolved = match client.resolve_wallet_input(&watched.wallet).await {
            Ok(resolved) => resolved,
            Err(error) => {
                if let Some(previous) = previous_rows.get(&config_index) {
                    let mut stale = previous.clone();
                    stale.paper_follow_enabled = watched.paper_follow_enabled;
                    rows.push(stale);
                    stale_wallets += 1;
                    warnings.push(format_status_error(
                        &format!("wallet resolve failed for {}", watched.wallet),
                        &error,
                    ));
                    continue;
                }
                dropped_wallets += 1;
                warnings.push(format_status_error(
                    &format!("wallet resolve failed for {}", watched.wallet),
                    &error,
                ));
                continue;
            }
        };

        let analysis = match build_wallet_analysis(client, config, &resolved.wallet, None).await {
            Ok(analysis) => analysis,
            Err(error) => {
                if let Some(previous) = previous_rows.get(&config_index) {
                    let mut stale = previous.clone();
                    stale.wallet = resolved.wallet;
                    stale.paper_follow_enabled = watched.paper_follow_enabled;
                    rows.push(stale);
                    stale_wallets += 1;
                    warnings.push(format_status_error(
                        &format!("wallet refresh failed for {}", watched.wallet),
                        &error,
                    ));
                    continue;
                }
                dropped_wallets += 1;
                warnings.push(format_status_error(
                    &format!("wallet refresh failed for {}", watched.wallet),
                    &error,
                ));
                continue;
            }
        };

        let label = watched
            .label
            .clone()
            .or(resolved.label)
            .or(resolved.username)
            .or(analysis.report.scorecard.user_name.clone())
            .unwrap_or_else(|| shorten_wallet(&resolved.wallet));

        persist_wallet_tracking(
            config,
            &label,
            &resolved.wallet,
            &analysis.report,
            &analysis.activities,
        )?;

        rows.push(ServiceWalletRow {
            config_index,
            wallet: resolved.wallet,
            label,
            paper_follow_enabled: watched.paper_follow_enabled,
            analysis,
        });
    }

    let paper = build_shared_paper_runtime(
        &rows
            .iter()
            .map(|row| PaperRuntimeWalletInput {
                wallet: row.wallet.clone(),
                label: row.label.clone(),
                paper_follow_enabled: row.paper_follow_enabled,
                analysis: row.analysis.clone(),
            })
            .collect::<Vec<_>>(),
        config,
    )?;

    Ok(ServiceCycleOutcome {
        rows,
        paper,
        stale_wallets,
        dropped_wallets,
        warnings,
    })
}

fn build_service_alerts(
    cycle: usize,
    config: &AlertConfig,
    summary: &PortfolioSimulationReport,
    executions: &[PortfolioSimulationExecution],
) -> Vec<ServiceAlertRecord> {
    executions
        .iter()
        .filter(|execution| should_alert_execution(config, execution))
        .map(|execution| ServiceAlertRecord {
            captured_at: now_ts(),
            cycle,
            category: alert_category(execution).to_owned(),
            severity: alert_severity(execution).to_owned(),
            message: format_execution_alert_message(execution),
            source_wallet: execution.source_wallet.clone(),
            source_label: execution.source_label.clone(),
            asset: execution.asset.clone(),
            title: execution.title.clone(),
            side: trade_side_label(execution.side),
            status: execution_status_label(execution.status).to_owned(),
            requested_usdc: execution.requested_usdc,
            filled_usdc: execution.filled_usdc,
            price: execution.price,
            reason: execution.reason.clone(),
            account_cash: summary.final_cash,
            account_equity: summary.final_equity,
        })
        .collect::<Vec<_>>()
}

fn should_alert_execution(config: &AlertConfig, execution: &PortfolioSimulationExecution) -> bool {
    match execution.status {
        SimulationExecutionStatus::Filled => config.alert_on_filled,
        SimulationExecutionStatus::Partial => config.alert_on_partial,
        SimulationExecutionStatus::Canceled => config.alert_on_canceled,
        SimulationExecutionStatus::Skipped => execution.reason.as_ref().is_some_and(|reason| {
            config
                .alert_on_skipped_reasons
                .iter()
                .any(|item| item == reason)
        }),
    }
}

fn emit_alerts(config: &AlertConfig, alerts: &[ServiceAlertRecord]) -> Result<()> {
    if config.print_to_stdout {
        for alert in alerts {
            println!(
                "[alert][{}][{}] {}",
                alert.severity.to_lowercase(),
                alert.category.to_lowercase(),
                alert.message
            );
        }
    }

    if config.desktop_notifications {
        for alert in alerts.iter().take(DESKTOP_ALERT_LIMIT_PER_CYCLE) {
            let title = format!("Mochi {} {}", alert.status, alert.side);
            let body = truncate_text(&alert.message, 180);
            let _ = Command::new(&config.desktop_command)
                .arg(title)
                .arg(body)
                .spawn();
        }
    }

    Ok(())
}

fn persist_service_status(config: &AppConfig, status: &ServiceStatusRecord) -> Result<()> {
    let service_dir = Path::new(&config.storage.data_dir).join("service");
    let history_dir = service_dir.join("history");
    fs::create_dir_all(&history_dir)?;

    let latest_path = service_dir.join("status.json");
    fs::write(latest_path, serde_json::to_string_pretty(status)?)?;

    let history_path = history_dir.join("status.jsonl");
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(history_path)?;
    writeln!(file, "{}", serde_json::to_string(status)?)?;
    Ok(())
}

fn persist_service_alerts(config: &AppConfig, alerts: &[ServiceAlertRecord]) -> Result<()> {
    if !config.alerts.persist_to_disk || alerts.is_empty() {
        return Ok(());
    }

    let service_dir = Path::new(&config.storage.data_dir).join("service");
    let alerts_dir = service_dir.join("alerts");
    let history_dir = alerts_dir.join("history");
    fs::create_dir_all(&history_dir)?;

    let latest_path = alerts_dir.join("latest.json");
    fs::write(latest_path, serde_json::to_string_pretty(alerts)?)?;

    let history_path = history_dir.join("alerts.jsonl");
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(history_path)?;
    for alert in alerts {
        writeln!(file, "{}", serde_json::to_string(alert)?)?;
    }

    Ok(())
}

fn format_execution_alert_message(execution: &PortfolioSimulationExecution) -> String {
    let label = execution
        .source_label
        .clone()
        .unwrap_or_else(|| shorten_wallet(&execution.source_wallet));
    let title = execution
        .title
        .clone()
        .unwrap_or_else(|| execution.asset.clone());
    let base = format!(
        "{} {} {} {:.2}/{:.2} @ {:.4} {}",
        label,
        trade_side_label(execution.side),
        execution_status_label(execution.status),
        execution.filled_usdc,
        execution.requested_usdc,
        execution.price,
        title
    );
    if let Some(reason) = &execution.reason {
        format!("{base} | {reason}")
    } else {
        base
    }
}

fn execution_status_label(status: SimulationExecutionStatus) -> &'static str {
    match status {
        SimulationExecutionStatus::Filled => "FILLED",
        SimulationExecutionStatus::Partial => "PARTIAL",
        SimulationExecutionStatus::Skipped => "SKIPPED",
        SimulationExecutionStatus::Canceled => "CANCELED",
    }
}

fn trade_side_label(side: crate::domain::TradeSide) -> String {
    match side {
        crate::domain::TradeSide::Buy => "BUY".to_owned(),
        crate::domain::TradeSide::Sell => "SELL".to_owned(),
        crate::domain::TradeSide::Unknown => "UNKNOWN".to_owned(),
    }
}

fn alert_category(execution: &PortfolioSimulationExecution) -> &'static str {
    match execution.status {
        SimulationExecutionStatus::Filled | SimulationExecutionStatus::Partial => "execution",
        SimulationExecutionStatus::Canceled => "cancellation",
        SimulationExecutionStatus::Skipped => "risk",
    }
}

fn alert_severity(execution: &PortfolioSimulationExecution) -> &'static str {
    match execution.status {
        SimulationExecutionStatus::Filled => "INFO",
        SimulationExecutionStatus::Partial => "WARN",
        SimulationExecutionStatus::Canceled | SimulationExecutionStatus::Skipped => "WARN",
    }
}

fn shorten_wallet(wallet: &str) -> String {
    if wallet.len() <= 14 {
        return wallet.to_owned();
    }

    format!("{}..{}", &wallet[..8], &wallet[wallet.len() - 4..])
}

fn truncate_text(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn format_status_error(prefix: &str, error: &anyhow::Error) -> String {
    let message = error
        .chain()
        .map(|part| part.to_string())
        .collect::<Vec<_>>()
        .join(" | ");
    format!("{}: {}", prefix, truncate_text(&message, 160))
}

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}
