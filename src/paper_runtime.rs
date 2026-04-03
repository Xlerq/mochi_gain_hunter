use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;

use crate::config::AppConfig;
use crate::domain::{PortfolioSimulationExecution, PortfolioSimulationReport};
use crate::reporting::WalletAnalysis;
use crate::simulation::{
    ForwardPaperJournalMetadata, SharedSimulationInput, advance_forward_paper_journal,
    can_resume_forward_paper_journal,
};
use crate::storage::{
    load_activity_log, load_activity_log_since, load_forward_paper_journal,
    persist_forward_paper_journal, persist_paper_account,
};

const FORWARD_JOURNAL_OVERLAP_SECONDS: i64 = 60 * 60;

#[derive(Debug, Clone)]
pub struct PaperRuntimeWalletInput {
    pub wallet: String,
    pub label: String,
    pub paper_follow_enabled: bool,
    pub analysis: WalletAnalysis,
}

#[derive(Debug, Clone)]
pub struct PaperRuntimeProgress {
    pub summary: PortfolioSimulationReport,
    pub recent_executions: Vec<PortfolioSimulationExecution>,
    pub new_executions: Vec<PortfolioSimulationExecution>,
    pub processed_activity_count: usize,
    pub resumed_journal: bool,
    pub replayed_history: bool,
}

pub fn build_shared_paper_runtime(
    rows: &[PaperRuntimeWalletInput],
    config: &AppConfig,
) -> Result<PaperRuntimeProgress> {
    let enabled_rows = rows
        .iter()
        .filter(|row| row.paper_follow_enabled)
        .collect::<Vec<_>>();

    if enabled_rows.is_empty() {
        return Ok(PaperRuntimeProgress {
            summary: PortfolioSimulationReport {
                tracked_wallets: 0,
                followed_trades: 0,
                ignored_trades: 0,
                closed_trades: 0,
                realized_pnl: 0.0,
                unrealized_pnl: 0.0,
                total_pnl: 0.0,
                final_cash: config.simulation.starting_cash,
                final_equity: config.simulation.starting_cash,
                starting_cash: config.simulation.starting_cash,
                tracked_from_timestamp: None,
                tracked_to_timestamp: None,
                deployed_cost_basis: 0.0,
                deployed_market_value: 0.0,
                cash_reserve_target: 0.0,
                skip_reasons: Vec::new(),
                open_positions: Vec::new(),
                closed_positions: Vec::new(),
                recent_executions: Vec::new(),
            },
            recent_executions: Vec::new(),
            new_executions: Vec::new(),
            processed_activity_count: 0,
            resumed_journal: false,
            replayed_history: false,
        });
    }

    let metadata = ForwardPaperJournalMetadata {
        enabled_wallets: enabled_rows
            .iter()
            .map(|row| row.wallet.clone())
            .collect::<Vec<_>>(),
        simulation_config: config.simulation.clone(),
    };
    let previous_journal = load_forward_paper_journal(config)?;
    let resumable = can_resume_forward_paper_journal(previous_journal.as_ref(), &metadata);
    let journal_start_timestamp = if resumable {
        previous_journal
            .as_ref()
            .and_then(|journal| journal.tracked_to_timestamp)
            .map(|timestamp| timestamp.saturating_sub(FORWARD_JOURNAL_OVERLAP_SECONDS))
    } else {
        None
    };

    let inputs = if resumable {
        let mut incremental_inputs = Vec::with_capacity(enabled_rows.len());
        for row in &enabled_rows {
            let activities = if let Some(start_timestamp) = journal_start_timestamp {
                load_activity_log_since(config, &row.wallet, start_timestamp)?
            } else {
                load_activity_log(config, &row.wallet)?
            };
            incremental_inputs.push(SharedSimulationInput {
                source_wallet: row.wallet.clone(),
                source_label: Some(row.label.clone()),
                activities,
                current_marks: row.analysis.current_marks.clone(),
            });
        }
        incremental_inputs
    } else {
        let mut full_history_inputs = Vec::with_capacity(enabled_rows.len());
        for row in &enabled_rows {
            full_history_inputs.push(SharedSimulationInput {
                source_wallet: row.wallet.clone(),
                source_label: Some(row.label.clone()),
                activities: load_activity_log(config, &row.wallet)?,
                current_marks: row.analysis.current_marks.clone(),
            });
        }
        full_history_inputs
    };

    let progress = advance_forward_paper_journal(
        previous_journal,
        &inputs,
        metadata,
        &config.simulation,
        now_ts(),
    );
    persist_forward_paper_journal(config, &progress.state, &progress.new_executions)?;
    persist_paper_account(config, &progress.report)?;

    Ok(PaperRuntimeProgress {
        summary: progress.report.clone(),
        recent_executions: progress.report.recent_executions.clone(),
        new_executions: progress.new_executions,
        processed_activity_count: progress.processed_activity_count,
        resumed_journal: progress.resumed,
        replayed_history: !progress.resumed,
    })
}

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}
