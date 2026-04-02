use std::fs;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::domain::{LeaderboardCategory, LeaderboardOrderBy, LeaderboardTimePeriod};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_data_api_base_url")]
    pub data_api_base_url: String,
    #[serde(default = "default_clob_api_base_url")]
    pub clob_api_base_url: String,
    #[serde(default = "default_gamma_api_base_url")]
    pub gamma_api_base_url: String,
    #[serde(default)]
    pub discover: DiscoverConfig,
    #[serde(default)]
    pub scoring: ScoringConfig,
    #[serde(default)]
    pub simulation: SimulationConfig,
    #[serde(default)]
    pub monitor: MonitorConfig,
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub backtest: BacktestConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            data_api_base_url: default_data_api_base_url(),
            clob_api_base_url: default_clob_api_base_url(),
            gamma_api_base_url: default_gamma_api_base_url(),
            discover: DiscoverConfig::default(),
            scoring: ScoringConfig::default(),
            simulation: SimulationConfig::default(),
            monitor: MonitorConfig::default(),
            storage: StorageConfig::default(),
            backtest: BacktestConfig::default(),
        }
    }
}

impl AppConfig {
    pub fn load_or_default(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let raw = fs::read_to_string(path)?;
        Ok(toml::from_str(&raw)?)
    }

    pub fn write_template(&self, path: &Path) -> Result<()> {
        self.write_to_path(path)
    }

    pub fn write_to_path(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let raw = toml::to_string_pretty(self)?;
        fs::write(path, raw)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverConfig {
    #[serde(default)]
    pub category: LeaderboardCategory,
    #[serde(default)]
    pub time_period: LeaderboardTimePeriod,
    #[serde(default)]
    pub order_by: LeaderboardOrderBy,
    #[serde(default = "default_candidate_count")]
    pub candidate_count: usize,
    #[serde(default = "default_activity_limit")]
    pub activity_limit: usize,
    #[serde(default = "default_positions_limit")]
    pub positions_limit: usize,
    #[serde(default = "default_closed_positions_limit")]
    pub closed_positions_limit: usize,
}

impl Default for DiscoverConfig {
    fn default() -> Self {
        Self {
            category: LeaderboardCategory::default(),
            time_period: LeaderboardTimePeriod::default(),
            order_by: LeaderboardOrderBy::default(),
            candidate_count: default_candidate_count(),
            activity_limit: default_activity_limit(),
            positions_limit: default_positions_limit(),
            closed_positions_limit: default_closed_positions_limit(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoringConfig {
    #[serde(default = "default_min_wallet_trades")]
    pub min_wallet_trades: usize,
    #[serde(default = "default_min_closed_positions")]
    pub min_closed_positions: usize,
    #[serde(default = "default_min_realized_pnl")]
    pub min_realized_pnl: f64,
    #[serde(default = "default_min_win_rate")]
    pub min_win_rate: f64,
    #[serde(default = "default_max_position_concentration")]
    pub max_position_concentration: f64,
    #[serde(default = "default_min_follow_score")]
    pub min_follow_score: f64,
    #[serde(default = "default_target_trade_count")]
    pub target_trade_count: usize,
    #[serde(default = "default_stale_after_days")]
    pub stale_after_days: u64,
    #[serde(default = "default_pnl_scale")]
    pub pnl_scale: f64,
    #[serde(default = "default_weight_realized_pnl")]
    pub weight_realized_pnl: f64,
    #[serde(default = "default_weight_open_pnl")]
    pub weight_open_pnl: f64,
    #[serde(default = "default_weight_consistency")]
    pub weight_consistency: f64,
    #[serde(default = "default_weight_activity")]
    pub weight_activity: f64,
    #[serde(default = "default_weight_freshness")]
    pub weight_freshness: f64,
    #[serde(default = "default_weight_leaderboard")]
    pub weight_leaderboard: f64,
    #[serde(default = "default_weight_concentration_penalty")]
    pub weight_concentration_penalty: f64,
}

impl Default for ScoringConfig {
    fn default() -> Self {
        Self {
            min_wallet_trades: default_min_wallet_trades(),
            min_closed_positions: default_min_closed_positions(),
            min_realized_pnl: default_min_realized_pnl(),
            min_win_rate: default_min_win_rate(),
            max_position_concentration: default_max_position_concentration(),
            min_follow_score: default_min_follow_score(),
            target_trade_count: default_target_trade_count(),
            stale_after_days: default_stale_after_days(),
            pnl_scale: default_pnl_scale(),
            weight_realized_pnl: default_weight_realized_pnl(),
            weight_open_pnl: default_weight_open_pnl(),
            weight_consistency: default_weight_consistency(),
            weight_activity: default_weight_activity(),
            weight_freshness: default_weight_freshness(),
            weight_leaderboard: default_weight_leaderboard(),
            weight_concentration_penalty: default_weight_concentration_penalty(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationConfig {
    #[serde(default = "default_follow_delay_secs")]
    pub follow_delay_secs: u64,
    #[serde(default = "default_slippage_bps")]
    pub slippage_bps: f64,
    #[serde(default = "default_wallet_scale")]
    pub wallet_scale: f64,
    #[serde(default = "default_max_trade_usdc")]
    pub max_trade_usdc: f64,
    #[serde(default = "default_minimum_trade_usdc")]
    pub minimum_trade_usdc: f64,
    #[serde(default = "default_starting_cash")]
    pub starting_cash: f64,
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            follow_delay_secs: default_follow_delay_secs(),
            slippage_bps: default_slippage_bps(),
            wallet_scale: default_wallet_scale(),
            max_trade_usdc: default_max_trade_usdc(),
            minimum_trade_usdc: default_minimum_trade_usdc(),
            starting_cash: default_starting_cash(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorConfig {
    #[serde(default = "default_poll_interval_secs")]
    pub poll_interval_secs: u64,
    #[serde(default = "default_recent_events_limit")]
    pub recent_events_limit: usize,
    #[serde(default = "default_focus_keywords")]
    pub focus_keywords: Vec<String>,
    #[serde(default = "default_watchlist")]
    pub wallets: Vec<WatchedWalletConfig>,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            poll_interval_secs: default_poll_interval_secs(),
            recent_events_limit: default_recent_events_limit(),
            focus_keywords: default_focus_keywords(),
            wallets: default_watchlist(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    #[serde(default = "default_data_dir")]
    pub data_dir: String,
    #[serde(default = "default_persist_snapshots")]
    pub persist_snapshots: bool,
    #[serde(default = "default_persist_activity")]
    pub persist_activity: bool,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            data_dir: default_data_dir(),
            persist_snapshots: default_persist_snapshots(),
            persist_activity: default_persist_activity(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestConfig {
    #[serde(default = "default_delay_grid_secs")]
    pub delay_grid_secs: Vec<u64>,
    #[serde(default = "default_slippage_grid_bps")]
    pub slippage_grid_bps: Vec<f64>,
    #[serde(default = "default_wallet_scale_grid")]
    pub wallet_scale_grid: Vec<f64>,
    #[serde(default = "default_backtest_max_results")]
    pub max_results: usize,
}

impl Default for BacktestConfig {
    fn default() -> Self {
        Self {
            delay_grid_secs: default_delay_grid_secs(),
            slippage_grid_bps: default_slippage_grid_bps(),
            wallet_scale_grid: default_wallet_scale_grid(),
            max_results: default_backtest_max_results(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchedWalletConfig {
    pub wallet: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default = "default_paper_follow_enabled")]
    pub paper_follow_enabled: bool,
}

fn default_data_api_base_url() -> String {
    "https://data-api.polymarket.com".to_owned()
}

fn default_clob_api_base_url() -> String {
    "https://clob.polymarket.com".to_owned()
}

fn default_gamma_api_base_url() -> String {
    "https://gamma-api.polymarket.com".to_owned()
}

fn default_candidate_count() -> usize {
    10
}

fn default_activity_limit() -> usize {
    200
}

fn default_positions_limit() -> usize {
    100
}

fn default_closed_positions_limit() -> usize {
    50
}

fn default_min_wallet_trades() -> usize {
    25
}

fn default_min_closed_positions() -> usize {
    8
}

fn default_min_realized_pnl() -> f64 {
    250.0
}

fn default_min_win_rate() -> f64 {
    0.45
}

fn default_max_position_concentration() -> f64 {
    0.65
}

fn default_min_follow_score() -> f64 {
    70.0
}

fn default_target_trade_count() -> usize {
    100
}

fn default_stale_after_days() -> u64 {
    14
}

fn default_pnl_scale() -> f64 {
    1_000.0
}

fn default_weight_realized_pnl() -> f64 {
    2.0
}

fn default_weight_open_pnl() -> f64 {
    0.75
}

fn default_weight_consistency() -> f64 {
    1.25
}

fn default_weight_activity() -> f64 {
    0.75
}

fn default_weight_freshness() -> f64 {
    0.5
}

fn default_weight_leaderboard() -> f64 {
    1.0
}

fn default_weight_concentration_penalty() -> f64 {
    0.75
}

fn default_follow_delay_secs() -> u64 {
    8
}

fn default_slippage_bps() -> f64 {
    35.0
}

fn default_wallet_scale() -> f64 {
    0.10
}

fn default_max_trade_usdc() -> f64 {
    50.0
}

fn default_minimum_trade_usdc() -> f64 {
    5.0
}

fn default_starting_cash() -> f64 {
    100.0
}

fn default_poll_interval_secs() -> u64 {
    12
}

fn default_recent_events_limit() -> usize {
    25
}

fn default_focus_keywords() -> Vec<String> {
    vec![
        "bitcoin".to_owned(),
        "btc".to_owned(),
        "ethereum".to_owned(),
        "eth".to_owned(),
        "solana".to_owned(),
        "crypto".to_owned(),
        "president".to_owned(),
        "trump".to_owned(),
        "election".to_owned(),
        "senate".to_owned(),
        "house".to_owned(),
        "white house".to_owned(),
        "fed".to_owned(),
        "sec".to_owned(),
    ]
}

fn default_watchlist() -> Vec<WatchedWalletConfig> {
    vec![
        WatchedWalletConfig {
            wallet: "0xde17f7144fbd0eddb2679132c10ff5e74b120988".to_owned(),
            label: Some("Attentive-Silica".to_owned()),
            paper_follow_enabled: true,
        },
        WatchedWalletConfig {
            wallet: "@gamblingisallyouneed".to_owned(),
            label: Some("GamblingIsAllYouNeed".to_owned()),
            paper_follow_enabled: true,
        },
    ]
}

fn default_data_dir() -> String {
    "data".to_owned()
}

fn default_persist_snapshots() -> bool {
    true
}

fn default_persist_activity() -> bool {
    true
}

fn default_delay_grid_secs() -> Vec<u64> {
    vec![2, 8, 20]
}

fn default_slippage_grid_bps() -> Vec<f64> {
    vec![10.0, 35.0, 75.0]
}

fn default_wallet_scale_grid() -> Vec<f64> {
    vec![0.05, 0.10, 0.20]
}

fn default_backtest_max_results() -> usize {
    8
}

fn default_paper_follow_enabled() -> bool {
    true
}
