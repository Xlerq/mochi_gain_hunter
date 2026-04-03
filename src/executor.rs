use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::Serialize;

use crate::config::{AppConfig, ExecutionMode};
use crate::domain::{PortfolioSimulationExecution, SimulationExecutionStatus, TradeSide};

#[derive(Debug, Clone)]
pub struct ExecutionBatchContext {
    pub cycle: usize,
    pub account_cash: f64,
    pub account_equity: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecutionIntent {
    pub intent_id: String,
    pub source_wallet: String,
    pub source_label: Option<String>,
    pub asset: String,
    pub title: Option<String>,
    pub side: TradeSide,
    pub expected_status: SimulationExecutionStatus,
    pub notional_usdc: f64,
    pub limit_price: f64,
    pub leader_timestamp: i64,
    pub decision_timestamp: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecutionReceipt {
    pub captured_at: i64,
    pub cycle: usize,
    pub mode: String,
    pub outcome: String,
    pub message: String,
    pub intent: ExecutionIntent,
    pub account_cash: f64,
    pub account_equity: f64,
}

pub trait ExecutionGateway {
    fn submit(
        &mut self,
        context: &ExecutionBatchContext,
        intents: &[ExecutionIntent],
    ) -> Result<Vec<ExecutionReceipt>>;
}

pub fn build_executor(config: &AppConfig) -> Box<dyn ExecutionGateway> {
    match config.execution.mode {
        ExecutionMode::Disabled => Box::new(DisabledExecutor::default()),
        ExecutionMode::Paper => Box::new(PaperExecutionRecorder::new(
            Path::new(&config.storage.data_dir),
            config.execution.print_to_stdout,
            config.execution.persist_to_disk,
        )),
    }
}

pub fn build_execution_intents(
    executions: &[PortfolioSimulationExecution],
    submit_partial: bool,
) -> Vec<ExecutionIntent> {
    executions
        .iter()
        .filter_map(|execution| match execution.status {
            SimulationExecutionStatus::Filled => Some(intent_from_execution(execution)),
            SimulationExecutionStatus::Partial if submit_partial => {
                Some(intent_from_execution(execution))
            }
            _ => None,
        })
        .collect::<Vec<_>>()
}

#[derive(Default)]
struct DisabledExecutor;

impl ExecutionGateway for DisabledExecutor {
    fn submit(
        &mut self,
        _context: &ExecutionBatchContext,
        _intents: &[ExecutionIntent],
    ) -> Result<Vec<ExecutionReceipt>> {
        Ok(Vec::new())
    }
}

struct PaperExecutionRecorder {
    data_dir: std::path::PathBuf,
    print_to_stdout: bool,
    persist_to_disk: bool,
}

impl PaperExecutionRecorder {
    fn new(data_dir: &Path, print_to_stdout: bool, persist_to_disk: bool) -> Self {
        Self {
            data_dir: data_dir.to_path_buf(),
            print_to_stdout,
            persist_to_disk,
        }
    }
}

impl ExecutionGateway for PaperExecutionRecorder {
    fn submit(
        &mut self,
        context: &ExecutionBatchContext,
        intents: &[ExecutionIntent],
    ) -> Result<Vec<ExecutionReceipt>> {
        let receipts = intents
            .iter()
            .map(|intent| ExecutionReceipt {
                captured_at: now_ts(),
                cycle: context.cycle,
                mode: "PAPER".to_owned(),
                outcome: "RECORDED".to_owned(),
                message: format!(
                    "paper executor recorded {} {} {:.2} @ {:.4}",
                    trade_side_label(intent.side),
                    intent.asset,
                    intent.notional_usdc,
                    intent.limit_price
                ),
                intent: intent.clone(),
                account_cash: context.account_cash,
                account_equity: context.account_equity,
            })
            .collect::<Vec<_>>();

        if self.print_to_stdout {
            for receipt in &receipts {
                println!(
                    "[executor][paper] {} {} {:.2} @ {:.4} {}",
                    trade_side_label(receipt.intent.side),
                    receipt.intent.asset,
                    receipt.intent.notional_usdc,
                    receipt.intent.limit_price,
                    receipt
                        .intent
                        .source_label
                        .clone()
                        .unwrap_or_else(|| shorten_wallet(&receipt.intent.source_wallet))
                );
            }
        }

        if self.persist_to_disk && !receipts.is_empty() {
            persist_receipts(&self.data_dir, &receipts)?;
        }

        Ok(receipts)
    }
}

fn intent_from_execution(execution: &PortfolioSimulationExecution) -> ExecutionIntent {
    ExecutionIntent {
        intent_id: format!(
            "{}:{}:{}:{}:{}:{:.6}",
            execution.source_wallet,
            execution.asset,
            trade_side_label(execution.side),
            execution.leader_timestamp,
            execution.timestamp,
            execution.filled_usdc
        ),
        source_wallet: execution.source_wallet.clone(),
        source_label: execution.source_label.clone(),
        asset: execution.asset.clone(),
        title: execution.title.clone(),
        side: execution.side,
        expected_status: execution.status,
        notional_usdc: execution.filled_usdc,
        limit_price: execution.price,
        leader_timestamp: execution.leader_timestamp,
        decision_timestamp: execution.timestamp,
    }
}

fn persist_receipts(data_dir: &Path, receipts: &[ExecutionReceipt]) -> Result<()> {
    let execution_dir = data_dir.join("execution");
    let history_dir = execution_dir.join("history");
    fs::create_dir_all(&history_dir)?;

    let latest_path = execution_dir.join("latest.json");
    fs::write(latest_path, serde_json::to_string_pretty(receipts)?)?;

    let history_path = history_dir.join("receipts.jsonl");
    let mut history_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(history_path)?;
    for receipt in receipts {
        writeln!(history_file, "{}", serde_json::to_string(receipt)?)?;
    }

    Ok(())
}

fn trade_side_label(side: TradeSide) -> &'static str {
    match side {
        TradeSide::Buy => "BUY",
        TradeSide::Sell => "SELL",
        TradeSide::Unknown => "UNKNOWN",
    }
}

fn shorten_wallet(wallet: &str) -> String {
    if wallet.len() <= 14 {
        return wallet.to_owned();
    }

    format!("{}..{}", &wallet[..8], &wallet[wallet.len() - 4..])
}

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use crate::domain::{PortfolioSimulationExecution, SimulationExecutionStatus, TradeSide};

    use super::build_execution_intents;

    #[test]
    fn builds_intents_only_for_actionable_executions() {
        let executions = vec![
            PortfolioSimulationExecution {
                source_wallet: "0x1".to_owned(),
                source_label: Some("One".to_owned()),
                asset: "asset-1".to_owned(),
                title: Some("Filled".to_owned()),
                leader_timestamp: 1,
                timestamp: 2,
                side: TradeSide::Buy,
                status: SimulationExecutionStatus::Filled,
                requested_usdc: 10.0,
                filled_usdc: 10.0,
                price: 0.5,
                usdc_size: 10.0,
                reason: None,
            },
            PortfolioSimulationExecution {
                source_wallet: "0x1".to_owned(),
                source_label: Some("One".to_owned()),
                asset: "asset-2".to_owned(),
                title: Some("Skipped".to_owned()),
                leader_timestamp: 3,
                timestamp: 4,
                side: TradeSide::Buy,
                status: SimulationExecutionStatus::Skipped,
                requested_usdc: 10.0,
                filled_usdc: 0.0,
                price: 0.5,
                usdc_size: 0.0,
                reason: Some("cash_reserve_blocked".to_owned()),
            },
        ];

        let intents = build_execution_intents(&executions, true);
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].asset, "asset-1");
        assert_eq!(intents[0].notional_usdc, 10.0);
    }
}
