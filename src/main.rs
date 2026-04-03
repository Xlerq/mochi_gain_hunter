mod app;
mod backtest;
mod config;
mod domain;
mod monitor;
mod paper_runtime;
mod polymarket;
mod reporting;
mod scoring;
mod service;
mod simulation;
mod storage;

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use serde::Serialize;

use crate::app::run_app;
use crate::backtest::run_backtest;
use crate::config::AppConfig;
use crate::domain::{LeaderboardCategory, LeaderboardOrderBy, LeaderboardTimePeriod, WalletReport};
use crate::monitor::run_monitor;
use crate::polymarket::PolymarketClient;
use crate::reporting::build_wallet_report;
use crate::service::run_service;

#[derive(Debug, Parser)]
#[command(name = "mochi_gain_hunter")]
#[command(
    version,
    about = "Polymarket wallet discovery and paper-follow simulator"
)]
struct Cli {
    #[arg(long, global = true, default_value = "config/default.toml")]
    config: PathBuf,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    InitConfig {
        #[arg(long)]
        force: bool,
    },
    Discover {
        #[arg(long)]
        category: Option<LeaderboardCategory>,
        #[arg(long)]
        time_period: Option<LeaderboardTimePeriod>,
        #[arg(long)]
        order_by: Option<LeaderboardOrderBy>,
        #[arg(long)]
        limit: Option<usize>,
    },
    InspectWallet {
        wallet: String,
    },
    SimulateFollow {
        wallet: String,
    },
    Monitor {
        wallet: Option<String>,
        #[arg(long)]
        plain: bool,
        #[arg(long)]
        cycles: Option<usize>,
    },
    Service {
        #[arg(long)]
        once: bool,
        #[arg(long)]
        cycles: Option<usize>,
    },
    BacktestWallet {
        wallet: String,
        #[arg(long)]
        top: Option<usize>,
        #[arg(long)]
        stored_only: bool,
    },
}

#[derive(Debug, Serialize)]
struct DiscoverOutput {
    config_path: String,
    reports: Vec<WalletReport>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::InitConfig { force }) => init_config(&cli.config, force),
        Some(Commands::Discover {
            category,
            time_period,
            order_by,
            limit,
        }) => discover_wallets(&cli.config, category, time_period, order_by, limit).await,
        Some(Commands::InspectWallet { wallet }) => inspect_wallet(&cli.config, &wallet).await,
        Some(Commands::SimulateFollow { wallet }) => simulate_wallet(&cli.config, &wallet).await,
        Some(Commands::Monitor {
            wallet,
            plain,
            cycles,
        }) => run_monitor(&cli.config, wallet.as_deref(), plain, cycles).await,
        Some(Commands::Service { once, cycles }) => run_service(&cli.config, once, cycles).await,
        Some(Commands::BacktestWallet {
            wallet,
            top,
            stored_only,
        }) => run_backtest(&cli.config, &wallet, top, stored_only).await,
        None => run_app(&cli.config).await,
    }
}

fn init_config(path: &Path, force: bool) -> Result<()> {
    if path.exists() && !force {
        bail!(
            "config file already exists at {}. Re-run with --force to overwrite it.",
            path.display()
        );
    }

    let config = AppConfig::default();
    config.write_template(path)?;
    println!("wrote config template to {}", path.display());
    Ok(())
}

async fn discover_wallets(
    config_path: &Path,
    category: Option<LeaderboardCategory>,
    time_period: Option<LeaderboardTimePeriod>,
    order_by: Option<LeaderboardOrderBy>,
    limit: Option<usize>,
) -> Result<()> {
    let mut config = AppConfig::load_or_default(config_path)?;
    if let Some(category) = category {
        config.discover.category = category;
    }
    if let Some(time_period) = time_period {
        config.discover.time_period = time_period;
    }
    if let Some(order_by) = order_by {
        config.discover.order_by = order_by;
    }
    if let Some(limit) = limit {
        config.discover.candidate_count = limit;
    }

    let client = PolymarketClient::new(&config)?;
    let leaderboard = client
        .leaderboard(
            config.discover.category,
            config.discover.time_period,
            config.discover.order_by,
            config.discover.candidate_count,
            0,
        )
        .await?;

    let mut reports = Vec::with_capacity(leaderboard.len());
    for entry in leaderboard {
        let wallet = entry.proxy_wallet.clone();
        reports.push(build_wallet_report(&client, &config, &wallet, Some(entry)).await?);
    }

    reports.sort_by(|left, right| {
        right
            .scorecard
            .score
            .partial_cmp(&left.scorecard.score)
            .unwrap_or(Ordering::Equal)
    });

    let output = DiscoverOutput {
        config_path: config_path.display().to_string(),
        reports,
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

async fn inspect_wallet(config_path: &Path, wallet: &str) -> Result<()> {
    let config = AppConfig::load_or_default(config_path)?;
    let client = PolymarketClient::new(&config)?;
    let resolved = client.resolve_wallet_input(wallet).await?;
    let report = build_wallet_report(&client, &config, &resolved.wallet, None).await?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

async fn simulate_wallet(config_path: &Path, wallet: &str) -> Result<()> {
    let config = AppConfig::load_or_default(config_path)?;
    let client = PolymarketClient::new(&config)?;
    let resolved = client.resolve_wallet_input(wallet).await?;
    let report = build_wallet_report(&client, &config, &resolved.wallet, None).await?;
    println!("{}", serde_json::to_string_pretty(&report.simulation)?);
    Ok(())
}
