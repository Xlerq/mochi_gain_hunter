use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, fmt};

use anyhow::{Result, anyhow};
use serde::Serialize;

use crate::config::{AppConfig, ExecutionConfig, ExecutionMode};
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
pub struct LiveOrderCandidate {
    pub clob_host: String,
    pub chain_id: u64,
    pub token_id: String,
    pub side: String,
    pub shares: f64,
    pub limit_price: f64,
    pub notional_usdc: f64,
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
    pub live_candidate: Option<LiveOrderCandidate>,
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
        ExecutionMode::Disabled => Box::new(DisabledExecutor),
        ExecutionMode::Paper => Box::new(PaperExecutionRecorder::new(
            Path::new(&config.storage.data_dir),
            config.execution.print_to_stdout,
            config.execution.persist_to_disk,
        )),
        ExecutionMode::LiveDryRun => match PolymarketLiveDryRunExecutor::new(
            Path::new(&config.storage.data_dir),
            &config.execution,
        ) {
            Ok(executor) => Box::new(executor),
            Err(error) => Box::new(BrokenLiveExecutor::new(
                Path::new(&config.storage.data_dir),
                config.execution.print_to_stdout,
                config.execution.persist_to_disk,
                format!("live dry-run executor is not ready: {error}"),
            )),
        },
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
                live_candidate: None,
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

struct BrokenLiveExecutor {
    data_dir: std::path::PathBuf,
    print_to_stdout: bool,
    persist_to_disk: bool,
    message: String,
}

impl BrokenLiveExecutor {
    fn new(data_dir: &Path, print_to_stdout: bool, persist_to_disk: bool, message: String) -> Self {
        Self {
            data_dir: data_dir.to_path_buf(),
            print_to_stdout,
            persist_to_disk,
            message,
        }
    }
}

impl ExecutionGateway for BrokenLiveExecutor {
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
                mode: "LIVE_DRY_RUN".to_owned(),
                outcome: "BLOCKED".to_owned(),
                message: self.message.clone(),
                intent: intent.clone(),
                account_cash: context.account_cash,
                account_equity: context.account_equity,
                live_candidate: None,
            })
            .collect::<Vec<_>>();

        if self.print_to_stdout {
            eprintln!("[executor][live-dry-run] {}", self.message);
        }

        if self.persist_to_disk && !receipts.is_empty() {
            persist_receipts(&self.data_dir, &receipts)?;
        }

        Ok(receipts)
    }
}

struct PolymarketLiveDryRunExecutor {
    data_dir: std::path::PathBuf,
    print_to_stdout: bool,
    persist_to_disk: bool,
    credentials: LiveCredentials,
    clob_host: String,
    chain_id: u64,
}

impl PolymarketLiveDryRunExecutor {
    fn new(data_dir: &Path, config: &ExecutionConfig) -> Result<Self> {
        Ok(Self {
            data_dir: data_dir.to_path_buf(),
            print_to_stdout: config.print_to_stdout,
            persist_to_disk: config.persist_to_disk,
            credentials: LiveCredentials::from_env(config)?,
            clob_host: config.clob_host.clone(),
            chain_id: config.chain_id,
        })
    }
}

impl ExecutionGateway for PolymarketLiveDryRunExecutor {
    fn submit(
        &mut self,
        context: &ExecutionBatchContext,
        intents: &[ExecutionIntent],
    ) -> Result<Vec<ExecutionReceipt>> {
        let receipts = intents
            .iter()
            .map(|intent| {
                let candidate = live_candidate_from_intent(&self.clob_host, self.chain_id, intent);
                ExecutionReceipt {
                    captured_at: now_ts(),
                    cycle: context.cycle,
                    mode: "LIVE_DRY_RUN".to_owned(),
                    outcome: "SIMULATED".to_owned(),
                    message: format!(
                        "live dry-run built {} {} {:.4} shares @ {:.4} using {} / {} / {} / {}",
                        trade_side_label(intent.side),
                        candidate.token_id,
                        candidate.shares,
                        candidate.limit_price,
                        self.credentials.private_key.label,
                        self.credentials.api_key.label,
                        self.credentials.secret.label,
                        self.credentials.passphrase.label
                    ),
                    intent: intent.clone(),
                    account_cash: context.account_cash,
                    account_equity: context.account_equity,
                    live_candidate: Some(candidate),
                }
            })
            .collect::<Vec<_>>();

        if self.print_to_stdout {
            for receipt in &receipts {
                if let Some(candidate) = &receipt.live_candidate {
                    println!(
                        "[executor][live-dry-run] {} {} {:.4} shares @ {:.4} wallet={} host={}",
                        trade_side_label(receipt.intent.side),
                        candidate.token_id,
                        candidate.shares,
                        candidate.limit_price,
                        shorten_wallet(&receipt.intent.source_wallet),
                        candidate.clob_host
                    );
                }
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

fn live_candidate_from_intent(
    clob_host: &str,
    chain_id: u64,
    intent: &ExecutionIntent,
) -> LiveOrderCandidate {
    LiveOrderCandidate {
        clob_host: clob_host.to_owned(),
        chain_id,
        token_id: intent.asset.clone(),
        side: trade_side_label(intent.side).to_owned(),
        shares: (intent.notional_usdc / intent.limit_price.max(0.000_001)).max(0.0),
        limit_price: intent.limit_price,
        notional_usdc: intent.notional_usdc,
    }
}

#[derive(Debug, Clone)]
struct SecretLabel {
    label: &'static str,
    value: String,
}

#[derive(Debug, Clone)]
struct LiveCredentials {
    private_key: SecretLabel,
    api_key: SecretLabel,
    secret: SecretLabel,
    passphrase: SecretLabel,
}

impl LiveCredentials {
    fn from_env(config: &ExecutionConfig) -> Result<Self> {
        Ok(Self {
            private_key: SecretLabel::load(&config.env_private_key, "private_key")?,
            api_key: SecretLabel::load(&config.env_api_key, "api_key")?,
            secret: SecretLabel::load(&config.env_secret, "secret")?,
            passphrase: SecretLabel::load(&config.env_passphrase, "passphrase")?,
        })
    }
}

impl SecretLabel {
    fn load(env_key: &str, label: &'static str) -> Result<Self> {
        let value = env::var(env_key)
            .map_err(|_| anyhow!("missing required env var {env_key} for live execution"))?;
        if value.trim().is_empty() {
            return Err(anyhow!("env var {env_key} is empty"));
        }

        Ok(Self { label, value })
    }
}

impl fmt::Display for SecretLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let _ = &self.value;
        write!(f, "{}", self.label)
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
