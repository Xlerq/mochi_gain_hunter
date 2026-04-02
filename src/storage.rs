use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::config::AppConfig;
use crate::domain::{
    PortfolioSimulationExecution, PortfolioSimulationReport, TradeSide, WalletActivity,
    WalletActivityType, WalletReport,
};
use crate::simulation::ForwardPaperJournalState;

#[derive(Debug, Serialize)]
struct StoredWalletSnapshot {
    captured_at: i64,
    label: String,
    wallet: String,
    report: WalletReport,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredActivityRecord {
    captured_at: i64,
    activity_key: String,
    wallet: String,
    activity: WalletActivity,
}

#[derive(Debug, Serialize)]
struct StoredPaperAccountSnapshot {
    captured_at: i64,
    report: PortfolioSimulationReport,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredForwardPaperJournalState {
    captured_at: i64,
    state: ForwardPaperJournalState,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredPaperExecutionRecord {
    captured_at: i64,
    execution: PortfolioSimulationExecution,
}

pub fn persist_wallet_tracking(
    config: &AppConfig,
    label: &str,
    wallet: &str,
    report: &WalletReport,
    activities: &[WalletActivity],
) -> Result<()> {
    let data_dir = PathBuf::from(&config.storage.data_dir);
    fs::create_dir_all(&data_dir)?;

    if config.storage.persist_snapshots {
        append_snapshot(&data_dir, label, wallet, report)?;
        write_latest_report(&data_dir, label, wallet, report)?;
    }

    if config.storage.persist_activity {
        append_activity_log(&data_dir, wallet, activities)?;
    }

    Ok(())
}

pub fn load_activity_log(config: &AppConfig, wallet: &str) -> Result<Vec<WalletActivity>> {
    let path = activity_log_path(Path::new(&config.storage.data_dir), wallet);
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut entries = Vec::new();

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let record: StoredActivityRecord = serde_json::from_str(&line)?;
        entries.push(record.activity);
    }

    entries.sort_by_key(|activity| activity.timestamp);
    Ok(entries)
}

pub fn load_activity_log_since(
    config: &AppConfig,
    wallet: &str,
    min_timestamp: i64,
) -> Result<Vec<WalletActivity>> {
    let path = activity_log_path(Path::new(&config.storage.data_dir), wallet);
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut entries = Vec::new();

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let record: StoredActivityRecord = serde_json::from_str(&line)?;
        if record.activity.timestamp >= min_timestamp {
            entries.push(record.activity);
        }
    }

    entries.sort_by_key(|activity| activity.timestamp);
    Ok(entries)
}

pub fn persist_paper_account(config: &AppConfig, report: &PortfolioSimulationReport) -> Result<()> {
    if !config.storage.persist_paper_account {
        return Ok(());
    }

    let data_dir = PathBuf::from(&config.storage.data_dir);
    fs::create_dir_all(&data_dir)?;

    let paper_dir = data_dir.join("paper_account");
    let history_dir = paper_dir.join("history");
    fs::create_dir_all(&history_dir)?;

    let snapshot = StoredPaperAccountSnapshot {
        captured_at: now_ts(),
        report: report.clone(),
    };

    let history_path = history_dir.join("shared_account.jsonl");
    let mut history_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(history_path)?;
    writeln!(history_file, "{}", serde_json::to_string(&snapshot)?)?;

    let latest_path = paper_dir.join("latest.json");
    fs::write(latest_path, serde_json::to_string_pretty(&snapshot)?)?;
    Ok(())
}

pub fn load_forward_paper_journal(config: &AppConfig) -> Result<Option<ForwardPaperJournalState>> {
    let path = Path::new(&config.storage.data_dir)
        .join("paper_account")
        .join("forward_state.json");
    if !path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(path)?;
    let stored: StoredForwardPaperJournalState = serde_json::from_str(&raw)?;
    Ok(Some(stored.state))
}

pub fn persist_forward_paper_journal(
    config: &AppConfig,
    state: &ForwardPaperJournalState,
    new_executions: &[PortfolioSimulationExecution],
) -> Result<()> {
    if !config.storage.persist_paper_account {
        return Ok(());
    }

    let paper_dir = Path::new(&config.storage.data_dir).join("paper_account");
    let history_dir = paper_dir.join("history");
    fs::create_dir_all(&history_dir)?;

    let state_record = StoredForwardPaperJournalState {
        captured_at: now_ts(),
        state: state.clone(),
    };
    let state_path = paper_dir.join("forward_state.json");
    fs::write(state_path, serde_json::to_string_pretty(&state_record)?)?;

    if !new_executions.is_empty() {
        let journal_path = history_dir.join("journal.jsonl");
        let mut journal_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(journal_path)?;

        for execution in new_executions {
            let record = StoredPaperExecutionRecord {
                captured_at: now_ts(),
                execution: execution.clone(),
            };
            writeln!(journal_file, "{}", serde_json::to_string(&record)?)?;
        }
    }

    Ok(())
}

fn append_snapshot(
    data_dir: &Path,
    label: &str,
    wallet: &str,
    report: &WalletReport,
) -> Result<()> {
    let history_dir = data_dir.join("history");
    fs::create_dir_all(&history_dir)?;
    let path = history_dir.join(format!("{wallet}.jsonl"));
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let record = StoredWalletSnapshot {
        captured_at: now_ts(),
        label: label.to_owned(),
        wallet: wallet.to_owned(),
        report: report.clone(),
    };
    writeln!(file, "{}", serde_json::to_string(&record)?)?;
    Ok(())
}

fn write_latest_report(
    data_dir: &Path,
    label: &str,
    wallet: &str,
    report: &WalletReport,
) -> Result<()> {
    let latest_dir = data_dir.join("latest");
    fs::create_dir_all(&latest_dir)?;
    let path = latest_dir.join(format!("{wallet}.json"));
    let record = StoredWalletSnapshot {
        captured_at: now_ts(),
        label: label.to_owned(),
        wallet: wallet.to_owned(),
        report: report.clone(),
    };
    fs::write(path, serde_json::to_string_pretty(&record)?)?;
    Ok(())
}

fn append_activity_log(data_dir: &Path, wallet: &str, activities: &[WalletActivity]) -> Result<()> {
    let activities_dir = data_dir.join("activities");
    fs::create_dir_all(&activities_dir)?;
    let path = activity_log_path(data_dir, wallet);

    let mut known_ids = HashSet::new();
    if path.exists() {
        let file = fs::File::open(&path)?;
        for line in BufReader::new(file).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(record) = serde_json::from_str::<StoredActivityRecord>(&line) {
                known_ids.insert(record.activity_key);
            }
        }
    }

    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    for activity in activities {
        if !matches!(activity.activity_type, WalletActivityType::Trade) {
            continue;
        }

        let activity_key = activity_key(wallet, activity);
        if known_ids.insert(activity_key.clone()) {
            let record = StoredActivityRecord {
                captured_at: now_ts(),
                activity_key,
                wallet: wallet.to_owned(),
                activity: activity.clone(),
            };
            writeln!(file, "{}", serde_json::to_string(&record)?)?;
        }
    }

    Ok(())
}

fn activity_log_path(data_dir: &Path, wallet: &str) -> PathBuf {
    data_dir.join("activities").join(format!("{wallet}.jsonl"))
}

fn activity_key(wallet: &str, activity: &WalletActivity) -> String {
    format!(
        "{}:{}:{}:{}:{}:{:.6}:{:.6}",
        wallet,
        activity.transaction_hash.as_deref().unwrap_or("nohash"),
        activity.timestamp,
        activity.asset,
        side_label(activity.side),
        activity.price,
        activity.usdc_size,
    )
}

fn side_label(side: Option<TradeSide>) -> &'static str {
    match side {
        Some(TradeSide::Buy) => "BUY",
        Some(TradeSide::Sell) => "SELL",
        _ => "UNKNOWN",
    }
}

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}
