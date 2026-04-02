use clap::ValueEnum;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ValueEnum, Default, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LeaderboardCategory {
    #[default]
    Overall,
    Politics,
    Sports,
    Crypto,
    Culture,
    Mentions,
    Weather,
    Economics,
    Tech,
    Finance,
}

impl LeaderboardCategory {
    pub const fn as_api_str(self) -> &'static str {
        match self {
            Self::Overall => "OVERALL",
            Self::Politics => "POLITICS",
            Self::Sports => "SPORTS",
            Self::Crypto => "CRYPTO",
            Self::Culture => "CULTURE",
            Self::Mentions => "MENTIONS",
            Self::Weather => "WEATHER",
            Self::Economics => "ECONOMICS",
            Self::Tech => "TECH",
            Self::Finance => "FINANCE",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ValueEnum, Default, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum LeaderboardTimePeriod {
    #[default]
    Month,
    Day,
    Week,
    All,
}

impl LeaderboardTimePeriod {
    pub const fn as_api_str(self) -> &'static str {
        match self {
            Self::Day => "DAY",
            Self::Week => "WEEK",
            Self::Month => "MONTH",
            Self::All => "ALL",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ValueEnum, Default, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum LeaderboardOrderBy {
    #[default]
    Pnl,
    Vol,
}

impl LeaderboardOrderBy {
    pub const fn as_api_str(self) -> &'static str {
        match self {
            Self::Pnl => "PNL",
            Self::Vol => "VOL",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WalletActivityType {
    Trade,
    Split,
    Merge,
    Redeem,
    Reward,
    Conversion,
    MakerRebate,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum TradeSide {
    Buy,
    Sell,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LeaderboardEntry {
    pub rank: String,
    pub proxy_wallet: String,
    #[serde(default)]
    pub user_name: Option<String>,
    #[serde(rename = "vol", default)]
    pub volume: f64,
    #[serde(default)]
    pub pnl: f64,
    #[serde(default)]
    pub profile_image: Option<String>,
    #[serde(default)]
    pub x_username: Option<String>,
    #[serde(default)]
    pub verified_badge: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletActivity {
    pub proxy_wallet: String,
    #[serde(default)]
    pub timestamp: i64,
    #[serde(default)]
    pub condition_id: String,
    #[serde(rename = "type")]
    pub activity_type: WalletActivityType,
    #[serde(default)]
    pub size: f64,
    #[serde(default)]
    pub usdc_size: f64,
    #[serde(default)]
    pub transaction_hash: Option<String>,
    #[serde(default)]
    pub price: f64,
    #[serde(default)]
    pub asset: String,
    #[serde(default)]
    pub side: Option<TradeSide>,
    #[serde(default)]
    pub outcome_index: Option<i64>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub event_slug: Option<String>,
    #[serde(default)]
    pub outcome: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub pseudonym: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Position {
    pub proxy_wallet: String,
    pub asset: String,
    pub condition_id: String,
    #[serde(default)]
    pub size: f64,
    #[serde(default)]
    pub avg_price: f64,
    #[serde(default)]
    pub initial_value: f64,
    #[serde(default)]
    pub current_value: f64,
    #[serde(default)]
    pub cash_pnl: f64,
    #[serde(default)]
    pub percent_pnl: f64,
    #[serde(default)]
    pub total_bought: f64,
    #[serde(default)]
    pub realized_pnl: f64,
    #[serde(default)]
    pub percent_realized_pnl: f64,
    #[serde(default)]
    pub cur_price: f64,
    #[serde(default)]
    pub redeemable: bool,
    #[serde(default)]
    pub mergeable: bool,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub event_slug: Option<String>,
    #[serde(default)]
    pub outcome: Option<String>,
    #[serde(default)]
    pub outcome_index: Option<i64>,
    #[serde(default)]
    pub opposite_outcome: Option<String>,
    #[serde(default)]
    pub opposite_asset: Option<String>,
    #[serde(default)]
    pub end_date: Option<String>,
    #[serde(default)]
    pub negative_risk: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClosedPosition {
    pub proxy_wallet: String,
    pub asset: String,
    pub condition_id: String,
    #[serde(default)]
    pub avg_price: f64,
    #[serde(default)]
    pub total_bought: f64,
    #[serde(default)]
    pub realized_pnl: f64,
    #[serde(default)]
    pub cur_price: f64,
    #[serde(default)]
    pub timestamp: i64,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub event_slug: Option<String>,
    #[serde(default)]
    pub outcome: Option<String>,
    #[serde(default)]
    pub outcome_index: Option<i64>,
    #[serde(default)]
    pub opposite_outcome: Option<String>,
    #[serde(default)]
    pub opposite_asset: Option<String>,
    #[serde(default)]
    pub end_date: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WalletAggregates {
    pub trade_count: usize,
    pub recent_trade_count: usize,
    pub open_position_count: usize,
    pub closed_position_count: usize,
    pub average_trade_usdc: f64,
    pub realized_pnl_total: f64,
    pub open_pnl_total: f64,
    pub leaderboard_pnl: f64,
    pub leaderboard_volume: f64,
    pub total_open_value: f64,
    pub top_position_ratio: f64,
    pub win_rate: f64,
    pub last_trade_timestamp: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScoreComponents {
    pub realized_pnl_score: f64,
    pub open_pnl_score: f64,
    pub consistency_score: f64,
    pub activity_score: f64,
    pub freshness_score: f64,
    pub leaderboard_score: f64,
    pub concentration_penalty: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FollowRecommendation {
    Ignore,
    Watch,
    ManualReview,
    PaperFollow,
}

#[derive(Debug, Clone, Serialize)]
pub struct WalletScorecard {
    pub wallet: String,
    pub user_name: Option<String>,
    pub leaderboard_rank: Option<String>,
    pub score: f64,
    pub eligible: bool,
    pub recommendation: FollowRecommendation,
    pub gating_reasons: Vec<String>,
    pub aggregates: WalletAggregates,
    pub components: ScoreComponents,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClosedSimulationTrade {
    pub source_wallet: Option<String>,
    pub source_label: Option<String>,
    pub asset: String,
    pub title: Option<String>,
    pub buy_timestamp: i64,
    pub sell_timestamp: i64,
    pub entry_price: f64,
    pub exit_price: f64,
    pub size: f64,
    pub pnl: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenSimulationPosition {
    pub source_wallet: Option<String>,
    pub source_label: Option<String>,
    pub asset: String,
    pub title: Option<String>,
    pub size: f64,
    pub avg_entry_price: f64,
    pub mark_price: f64,
    pub unrealized_pnl: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SimulationReport {
    pub wallet: String,
    pub followed_trades: usize,
    pub ignored_trades: usize,
    pub closed_trades: usize,
    pub win_rate: f64,
    pub realized_pnl: f64,
    pub unrealized_pnl: f64,
    pub total_pnl: f64,
    pub final_cash: f64,
    pub final_equity: f64,
    pub starting_cash: f64,
    pub tracked_from_timestamp: Option<i64>,
    pub tracked_to_timestamp: Option<i64>,
    pub deployed_cost_basis: f64,
    pub deployed_market_value: f64,
    pub cash_reserve_target: f64,
    pub skip_reasons: Vec<SimulationSkipReasonCount>,
    pub open_positions: Vec<OpenSimulationPosition>,
    pub closed_positions: Vec<ClosedSimulationTrade>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationSkipReasonCount {
    pub reason: String,
    pub count: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SimulationExecutionStatus {
    Filled,
    Partial,
    Skipped,
    Canceled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioSimulationExecution {
    pub source_wallet: String,
    pub source_label: Option<String>,
    pub asset: String,
    pub title: Option<String>,
    pub leader_timestamp: i64,
    pub timestamp: i64,
    pub side: TradeSide,
    pub status: SimulationExecutionStatus,
    pub requested_usdc: f64,
    pub filled_usdc: f64,
    pub price: f64,
    pub usdc_size: f64,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PortfolioSimulationPosition {
    pub source_wallet: String,
    pub source_label: Option<String>,
    pub asset: String,
    pub title: Option<String>,
    pub size: f64,
    pub avg_entry_price: f64,
    pub mark_price: f64,
    pub unrealized_pnl: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PortfolioSimulationReport {
    pub tracked_wallets: usize,
    pub followed_trades: usize,
    pub ignored_trades: usize,
    pub closed_trades: usize,
    pub realized_pnl: f64,
    pub unrealized_pnl: f64,
    pub total_pnl: f64,
    pub final_cash: f64,
    pub final_equity: f64,
    pub starting_cash: f64,
    pub tracked_from_timestamp: Option<i64>,
    pub tracked_to_timestamp: Option<i64>,
    pub deployed_cost_basis: f64,
    pub deployed_market_value: f64,
    pub cash_reserve_target: f64,
    pub skip_reasons: Vec<SimulationSkipReasonCount>,
    pub open_positions: Vec<PortfolioSimulationPosition>,
    pub closed_positions: Vec<ClosedSimulationTrade>,
    pub recent_executions: Vec<PortfolioSimulationExecution>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WalletReport {
    pub scorecard: WalletScorecard,
    pub simulation: SimulationReport,
}
