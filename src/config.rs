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
    pub http: HttpConfig,
    #[serde(default)]
    pub discover: DiscoverConfig,
    #[serde(default)]
    pub scoring: ScoringConfig,
    #[serde(default)]
    pub simulation: SimulationConfig,
    #[serde(default)]
    pub monitor: MonitorConfig,
    #[serde(default)]
    pub service: ServiceConfig,
    #[serde(default)]
    pub alerts: AlertConfig,
    #[serde(default)]
    pub execution: ExecutionConfig,
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
            http: HttpConfig::default(),
            discover: DiscoverConfig::default(),
            scoring: ScoringConfig::default(),
            simulation: SimulationConfig::default(),
            monitor: MonitorConfig::default(),
            service: ServiceConfig::default(),
            alerts: AlertConfig::default(),
            execution: ExecutionConfig::default(),
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
pub struct HttpConfig {
    #[serde(default = "default_request_timeout_secs")]
    pub request_timeout_secs: u64,
    #[serde(default = "default_connect_timeout_secs")]
    pub connect_timeout_secs: u64,
    #[serde(default = "default_retry_attempts")]
    pub retry_attempts: usize,
    #[serde(default = "default_retry_backoff_ms")]
    pub retry_backoff_ms: u64,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            request_timeout_secs: default_request_timeout_secs(),
            connect_timeout_secs: default_connect_timeout_secs(),
            retry_attempts: default_retry_attempts(),
            retry_backoff_ms: default_retry_backoff_ms(),
        }
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SimulationConfig {
    #[serde(default = "default_follow_delay_secs")]
    pub follow_delay_secs: u64,
    #[serde(default = "default_slippage_bps")]
    pub slippage_bps: f64,
    #[serde(default = "default_impact_slippage_bps")]
    pub impact_slippage_bps: f64,
    #[serde(default = "default_wallet_scale")]
    pub wallet_scale: f64,
    #[serde(default = "default_max_trade_usdc")]
    pub max_trade_usdc: f64,
    #[serde(default = "default_minimum_trade_usdc")]
    pub minimum_trade_usdc: f64,
    #[serde(default = "default_min_leader_trade_usdc")]
    pub min_leader_trade_usdc: f64,
    #[serde(default = "default_starting_cash")]
    pub starting_cash: f64,
    #[serde(default = "default_cash_reserve_ratio")]
    pub cash_reserve_ratio: f64,
    #[serde(default = "default_max_total_exposure_ratio")]
    pub max_total_exposure_ratio: f64,
    #[serde(default = "default_max_position_exposure_ratio")]
    pub max_position_exposure_ratio: f64,
    #[serde(default = "default_max_wallet_exposure_ratio")]
    pub max_wallet_exposure_ratio: f64,
    #[serde(default = "default_max_open_positions")]
    pub max_open_positions: usize,
    #[serde(default = "default_taker_fee_bps")]
    pub taker_fee_bps: f64,
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            follow_delay_secs: default_follow_delay_secs(),
            slippage_bps: default_slippage_bps(),
            impact_slippage_bps: default_impact_slippage_bps(),
            wallet_scale: default_wallet_scale(),
            max_trade_usdc: default_max_trade_usdc(),
            minimum_trade_usdc: default_minimum_trade_usdc(),
            min_leader_trade_usdc: default_min_leader_trade_usdc(),
            starting_cash: default_starting_cash(),
            cash_reserve_ratio: default_cash_reserve_ratio(),
            max_total_exposure_ratio: default_max_total_exposure_ratio(),
            max_position_exposure_ratio: default_max_position_exposure_ratio(),
            max_wallet_exposure_ratio: default_max_wallet_exposure_ratio(),
            max_open_positions: default_max_open_positions(),
            taker_fee_bps: default_taker_fee_bps(),
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
pub struct ServiceConfig {
    #[serde(default = "default_service_poll_interval_secs")]
    pub poll_interval_secs: u64,
    #[serde(default = "default_service_print_heartbeat")]
    pub print_heartbeat: bool,
    #[serde(default = "default_service_suppress_replay_alerts")]
    pub suppress_replay_alerts: bool,
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            poll_interval_secs: default_service_poll_interval_secs(),
            print_heartbeat: default_service_print_heartbeat(),
            suppress_replay_alerts: default_service_suppress_replay_alerts(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertConfig {
    #[serde(default = "default_alert_print_to_stdout")]
    pub print_to_stdout: bool,
    #[serde(default = "default_alert_persist_to_disk")]
    pub persist_to_disk: bool,
    #[serde(default = "default_alert_desktop_notifications")]
    pub desktop_notifications: bool,
    #[serde(default = "default_alert_desktop_command")]
    pub desktop_command: String,
    #[serde(default = "default_alert_on_filled")]
    pub alert_on_filled: bool,
    #[serde(default = "default_alert_on_partial")]
    pub alert_on_partial: bool,
    #[serde(default = "default_alert_on_canceled")]
    pub alert_on_canceled: bool,
    #[serde(default = "default_alert_on_skipped_reasons")]
    pub alert_on_skipped_reasons: Vec<String>,
}

impl Default for AlertConfig {
    fn default() -> Self {
        Self {
            print_to_stdout: default_alert_print_to_stdout(),
            persist_to_disk: default_alert_persist_to_disk(),
            desktop_notifications: default_alert_desktop_notifications(),
            desktop_command: default_alert_desktop_command(),
            alert_on_filled: default_alert_on_filled(),
            alert_on_partial: default_alert_on_partial(),
            alert_on_canceled: default_alert_on_canceled(),
            alert_on_skipped_reasons: default_alert_on_skipped_reasons(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExecutionMode {
    Disabled,
    Paper,
    LiveDryRun,
}

impl Default for ExecutionMode {
    fn default() -> Self {
        Self::Paper
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionConfig {
    #[serde(default)]
    pub mode: ExecutionMode,
    #[serde(default = "default_execution_print_to_stdout")]
    pub print_to_stdout: bool,
    #[serde(default = "default_execution_persist_to_disk")]
    pub persist_to_disk: bool,
    #[serde(default = "default_execution_submit_partial")]
    pub submit_partial: bool,
    #[serde(default = "default_execution_clob_host")]
    pub clob_host: String,
    #[serde(default = "default_execution_chain_id")]
    pub chain_id: u64,
    #[serde(default = "default_execution_env_private_key")]
    pub env_private_key: String,
    #[serde(default = "default_execution_env_api_key")]
    pub env_api_key: String,
    #[serde(default = "default_execution_env_secret")]
    pub env_secret: String,
    #[serde(default = "default_execution_env_passphrase")]
    pub env_passphrase: String,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            mode: ExecutionMode::default(),
            print_to_stdout: default_execution_print_to_stdout(),
            persist_to_disk: default_execution_persist_to_disk(),
            submit_partial: default_execution_submit_partial(),
            clob_host: default_execution_clob_host(),
            chain_id: default_execution_chain_id(),
            env_private_key: default_execution_env_private_key(),
            env_api_key: default_execution_env_api_key(),
            env_secret: default_execution_env_secret(),
            env_passphrase: default_execution_env_passphrase(),
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
    #[serde(default = "default_persist_paper_account")]
    pub persist_paper_account: bool,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            data_dir: default_data_dir(),
            persist_snapshots: default_persist_snapshots(),
            persist_activity: default_persist_activity(),
            persist_paper_account: default_persist_paper_account(),
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

fn default_request_timeout_secs() -> u64 {
    30
}

fn default_connect_timeout_secs() -> u64 {
    10
}

fn default_retry_attempts() -> usize {
    3
}

fn default_retry_backoff_ms() -> u64 {
    750
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

fn default_impact_slippage_bps() -> f64 {
    20.0
}

fn default_wallet_scale() -> f64 {
    0.10
}

fn default_max_trade_usdc() -> f64 {
    50.0
}

fn default_minimum_trade_usdc() -> f64 {
    2.0
}

fn default_min_leader_trade_usdc() -> f64 {
    0.0
}

fn default_starting_cash() -> f64 {
    100.0
}

fn default_cash_reserve_ratio() -> f64 {
    0.10
}

fn default_max_total_exposure_ratio() -> f64 {
    0.90
}

fn default_max_position_exposure_ratio() -> f64 {
    0.35
}

fn default_max_wallet_exposure_ratio() -> f64 {
    0.50
}

fn default_max_open_positions() -> usize {
    6
}

fn default_taker_fee_bps() -> f64 {
    50.0
}

fn default_poll_interval_secs() -> u64 {
    12
}

fn default_recent_events_limit() -> usize {
    25
}

fn default_service_poll_interval_secs() -> u64 {
    15
}

fn default_service_print_heartbeat() -> bool {
    true
}

fn default_service_suppress_replay_alerts() -> bool {
    true
}

fn default_alert_print_to_stdout() -> bool {
    true
}

fn default_alert_persist_to_disk() -> bool {
    true
}

fn default_alert_desktop_notifications() -> bool {
    false
}

fn default_alert_desktop_command() -> String {
    "notify-send".to_owned()
}

fn default_alert_on_filled() -> bool {
    true
}

fn default_alert_on_partial() -> bool {
    true
}

fn default_alert_on_canceled() -> bool {
    true
}

fn default_alert_on_skipped_reasons() -> Vec<String> {
    vec![
        "cash_reserve_blocked".to_owned(),
        "total_exposure_limit".to_owned(),
        "wallet_exposure_limit".to_owned(),
        "position_exposure_limit".to_owned(),
        "max_open_positions_reached".to_owned(),
        "leader_unwound_before_fill".to_owned(),
        "follower_size_too_small_after_fees".to_owned(),
    ]
}

fn default_execution_print_to_stdout() -> bool {
    true
}

fn default_execution_persist_to_disk() -> bool {
    true
}

fn default_execution_submit_partial() -> bool {
    true
}

fn default_execution_clob_host() -> String {
    "https://clob.polymarket.com".to_owned()
}

fn default_execution_chain_id() -> u64 {
    137
}

fn default_execution_env_private_key() -> String {
    "POLYMARKET_PRIVATE_KEY".to_owned()
}

fn default_execution_env_api_key() -> String {
    "POLYMARKET_API_KEY".to_owned()
}

fn default_execution_env_secret() -> String {
    "POLYMARKET_SECRET".to_owned()
}

fn default_execution_env_passphrase() -> String {
    "POLYMARKET_PASSPHRASE".to_owned()
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

fn default_persist_paper_account() -> bool {
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
