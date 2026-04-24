use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use futures_util::{StreamExt, TryStreamExt, stream};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap};
use ratatui::{Frame, Terminal};
use tokio::task::JoinHandle;

use crate::config::{AppConfig, WatchedWalletConfig};
use crate::domain::{
    FollowRecommendation, LeaderboardCategory, LeaderboardEntry, LeaderboardOrderBy,
    LeaderboardTimePeriod, PortfolioSimulationExecution, PortfolioSimulationPosition,
    PortfolioSimulationReport, SimulationExecutionStatus, TradeSide, WalletActivity,
    WalletActivityType, WalletReport,
};
use crate::paper_runtime::{PaperRuntimeWalletInput, build_shared_paper_runtime};
use crate::polymarket::PolymarketClient;
use crate::reporting::{
    ResolvedWalletAnalysis, WalletAnalysis, build_resolved_wallet_analysis, build_wallet_analysis,
    build_wallet_report,
};
use crate::storage::persist_wallet_tracking;

const LEADERBOARD_CATEGORIES: [LeaderboardCategory; 3] = [
    LeaderboardCategory::Politics,
    LeaderboardCategory::Crypto,
    LeaderboardCategory::Overall,
];
const LEADERBOARD_TIME_PERIODS: [LeaderboardTimePeriod; 4] = [
    LeaderboardTimePeriod::Day,
    LeaderboardTimePeriod::Week,
    LeaderboardTimePeriod::Month,
    LeaderboardTimePeriod::All,
];
const LEADERBOARD_ORDERINGS: [LeaderboardOrderBy; 2] =
    [LeaderboardOrderBy::Pnl, LeaderboardOrderBy::Vol];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppTab {
    Leaderboard,
    Watchlist,
    Wallet,
    Paper,
}

impl AppTab {
    const ALL: [Self; 4] = [
        Self::Leaderboard,
        Self::Watchlist,
        Self::Wallet,
        Self::Paper,
    ];

    const fn title(self) -> &'static str {
        match self {
            Self::Leaderboard => "Leaderboard",
            Self::Watchlist => "Watchlist",
            Self::Wallet => "Wallet",
            Self::Paper => "Paper",
        }
    }
}

#[derive(Debug, Clone)]
struct LeaderboardRow {
    entry: LeaderboardEntry,
    wallet: String,
    label: String,
    report: WalletReport,
    watched: bool,
    paper_follow_enabled: bool,
}

#[derive(Debug, Clone)]
struct WatchedWalletRow {
    config_index: usize,
    wallet: String,
    label: String,
    paper_follow_enabled: bool,
    analysis: WalletAnalysis,
}

#[derive(Debug, Clone)]
struct WalletDetail {
    wallet: String,
    label: String,
    source: String,
    watched: bool,
    paper_follow_enabled: bool,
    analysis: WalletAnalysis,
}

#[derive(Debug, Clone)]
struct PaperWalletRow {
    label: String,
    wallet: String,
    recommendation: String,
    total_pnl: f64,
    final_equity: f64,
    followed_trades: usize,
    closed_trades: usize,
}

#[derive(Debug, Clone)]
struct PaperDashboard {
    summary: PortfolioSimulationReport,
    wallets: Vec<PaperWalletRow>,
    recent_executions: Vec<PortfolioSimulationExecution>,
    processed_activity_count: usize,
    resumed_journal: bool,
}

#[derive(Debug)]
struct WatchlistRefreshResult {
    rows: Vec<WatchedWalletRow>,
    paper_dashboard: PaperDashboard,
    refreshed_at: i64,
    stale_wallets: usize,
    dropped_wallets: usize,
}

#[derive(Debug)]
struct WatchlistRefreshOutcome {
    config_index: usize,
    watched: WatchedWalletConfig,
    result: Result<ResolvedWalletAnalysis>,
}

#[derive(Debug)]
struct LeaderboardRefreshResult {
    rows: Vec<LeaderboardRow>,
    refreshed_at: i64,
}

#[derive(Debug)]
struct LeaderboardRefreshOutcome {
    index: usize,
    entry: LeaderboardEntry,
    wallet: String,
    report: WalletReport,
}

impl PaperDashboard {
    fn empty(starting_cash: f64) -> Self {
        Self {
            summary: PortfolioSimulationReport {
                tracked_wallets: 0,
                followed_trades: 0,
                ignored_trades: 0,
                closed_trades: 0,
                realized_pnl: 0.0,
                unrealized_pnl: 0.0,
                total_pnl: 0.0,
                final_cash: starting_cash,
                final_equity: starting_cash,
                starting_cash,
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
            wallets: Vec::new(),
            recent_executions: Vec::new(),
            processed_activity_count: 0,
            resumed_journal: false,
        }
    }
}

#[derive(Debug, Clone)]
enum SelectionContext {
    Leaderboard(usize),
    Watchlist(usize),
}

#[derive(Debug, Clone)]
enum WalletAction {
    InspectWallet,
    AddToWatchlist,
    RemoveFromWatchlist,
    StartPaperFollow,
    StopPaperFollow,
    RefreshData,
}

impl WalletAction {
    const fn label(&self) -> &'static str {
        match self {
            Self::InspectWallet => "Inspect wallet",
            Self::AddToWatchlist => "Add to watchlist",
            Self::RemoveFromWatchlist => "Remove from watchlist",
            Self::StartPaperFollow => "Start paper-follow",
            Self::StopPaperFollow => "Stop paper-follow",
            Self::RefreshData => "Refresh data",
        }
    }
}

#[derive(Debug, Clone)]
struct ActionMenuState {
    context: SelectionContext,
    selected: usize,
    actions: Vec<WalletAction>,
}

struct AppState {
    config_path: PathBuf,
    config: AppConfig,
    active_tab: AppTab,
    status_message: String,
    last_refresh_timestamp: Option<i64>,
    leaderboard_rows: Vec<LeaderboardRow>,
    leaderboard_selected: usize,
    watchlist_rows: Vec<WatchedWalletRow>,
    watchlist_selected: usize,
    wallet_detail: Option<WalletDetail>,
    paper_dashboard: PaperDashboard,
    action_menu: Option<ActionMenuState>,
    refresh_watchlist: bool,
    refresh_leaderboard: bool,
}

impl AppState {
    fn new(config_path: &Path, config: AppConfig) -> Self {
        let starting_cash = config.simulation.starting_cash;
        Self {
            config_path: config_path.to_path_buf(),
            config,
            active_tab: AppTab::Leaderboard,
            status_message: "starting app".to_owned(),
            last_refresh_timestamp: None,
            leaderboard_rows: Vec::new(),
            leaderboard_selected: 0,
            watchlist_rows: Vec::new(),
            watchlist_selected: 0,
            wallet_detail: None,
            paper_dashboard: PaperDashboard::empty(starting_cash),
            action_menu: None,
            refresh_watchlist: true,
            refresh_leaderboard: true,
        }
    }

    fn next_tab(&mut self) {
        let index = AppTab::ALL
            .iter()
            .position(|tab| *tab == self.active_tab)
            .unwrap_or_default();
        self.active_tab = AppTab::ALL[(index + 1) % AppTab::ALL.len()];
    }

    fn previous_tab(&mut self) {
        let index = AppTab::ALL
            .iter()
            .position(|tab| *tab == self.active_tab)
            .unwrap_or_default();
        self.active_tab = AppTab::ALL[(index + AppTab::ALL.len() - 1) % AppTab::ALL.len()];
    }

    fn move_selection(&mut self, delta: isize) {
        match self.active_tab {
            AppTab::Leaderboard => {
                self.leaderboard_selected = move_index(
                    self.leaderboard_selected,
                    self.leaderboard_rows.len(),
                    delta,
                );
            }
            AppTab::Watchlist => {
                self.watchlist_selected =
                    move_index(self.watchlist_selected, self.watchlist_rows.len(), delta);
            }
            AppTab::Wallet | AppTab::Paper => {}
        }
    }

    fn jump_to_start(&mut self) {
        match self.active_tab {
            AppTab::Leaderboard => self.leaderboard_selected = 0,
            AppTab::Watchlist => self.watchlist_selected = 0,
            AppTab::Wallet | AppTab::Paper => {}
        }
    }

    fn jump_to_end(&mut self) {
        match self.active_tab {
            AppTab::Leaderboard => {
                self.leaderboard_selected = self.leaderboard_rows.len().saturating_sub(1);
            }
            AppTab::Watchlist => {
                self.watchlist_selected = self.watchlist_rows.len().saturating_sub(1);
            }
            AppTab::Wallet | AppTab::Paper => {}
        }
    }

    fn selected_context(&self) -> Option<SelectionContext> {
        match self.active_tab {
            AppTab::Leaderboard if !self.leaderboard_rows.is_empty() => {
                Some(SelectionContext::Leaderboard(self.leaderboard_selected))
            }
            AppTab::Watchlist if !self.watchlist_rows.is_empty() => {
                Some(SelectionContext::Watchlist(self.watchlist_selected))
            }
            AppTab::Wallet | AppTab::Paper => None,
            _ => None,
        }
    }
}

pub async fn run_app(config_path: &Path) -> Result<()> {
    let config = AppConfig::load_or_default(config_path)?;
    let client = PolymarketClient::new(&config)?;
    let mut state = AppState::new(config_path, config);

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut refresh_deadline = Instant::now() - Duration::from_secs(3600);
    let mut watchlist_task: Option<JoinHandle<Result<WatchlistRefreshResult>>> = None;
    let mut leaderboard_task: Option<JoinHandle<Result<LeaderboardRefreshResult>>> = None;

    let run_result = async {
        loop {
            if watchlist_task
                .as_ref()
                .is_some_and(|task| task.is_finished())
            {
                let task = watchlist_task.take().expect("checked above");
                match task.await {
                    Ok(Ok(result)) => apply_watchlist_refresh(&mut state, result),
                    Ok(Err(error)) => {
                        state.status_message =
                            format_status_error("watchlist refresh failed", &error);
                    }
                    Err(error) => {
                        state.status_message = format!("watchlist refresh task failed: {error}");
                    }
                }
            }

            if leaderboard_task
                .as_ref()
                .is_some_and(|task| task.is_finished())
            {
                let task = leaderboard_task.take().expect("checked above");
                match task.await {
                    Ok(Ok(result)) => apply_leaderboard_refresh(&mut state, result),
                    Ok(Err(error)) => {
                        state.status_message =
                            format_status_error("leaderboard refresh failed", &error);
                    }
                    Err(error) => {
                        state.status_message = format!("leaderboard refresh task failed: {error}");
                    }
                }
            }

            let should_refresh_watchlist = state.refresh_watchlist
                || refresh_deadline.elapsed()
                    >= Duration::from_secs(state.config.monitor.poll_interval_secs);
            if should_refresh_watchlist && watchlist_task.is_none() {
                state.refresh_watchlist = false;
                state.status_message =
                    "refreshing watchlist and paper book in background".to_owned();
                watchlist_task = Some(spawn_watchlist_refresh(client.clone(), &state));
                refresh_deadline = Instant::now();
            }

            if state.refresh_leaderboard && leaderboard_task.is_none() {
                state.refresh_leaderboard = false;
                state.status_message = "refreshing leaderboard candidates in background".to_owned();
                leaderboard_task = Some(spawn_leaderboard_refresh(client.clone(), &state));
            }

            terminal.draw(|frame| draw_app(frame, &state))?;

            if event::poll(Duration::from_millis(200))?
                && let Event::Key(key) = event::read()?
            {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                if let Some(menu) = &mut state.action_menu {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('q') => state.action_menu = None,
                        KeyCode::Up | KeyCode::Char('k') => {
                            menu.selected = move_index(menu.selected, menu.actions.len(), -1)
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            menu.selected = move_index(menu.selected, menu.actions.len(), 1)
                        }
                        KeyCode::Enter => {
                            let action = menu.actions.get(menu.selected).cloned();
                            let context = menu.context.clone();
                            state.action_menu = None;
                            if let Some(action) = action
                                && let Err(error) =
                                    perform_action(&client, &mut state, context, action).await
                            {
                                state.status_message =
                                    format_status_error("wallet action failed", &error);
                            }
                        }
                        _ => {}
                    }
                    continue;
                }

                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Left | KeyCode::BackTab | KeyCode::Char('h') => {
                        state.previous_tab();
                    }
                    KeyCode::Right | KeyCode::Tab | KeyCode::Char('l') => {
                        state.next_tab();
                    }
                    KeyCode::Up | KeyCode::Char('k') => state.move_selection(-1),
                    KeyCode::Down | KeyCode::Char('j') => state.move_selection(1),
                    KeyCode::Char('g') => state.jump_to_start(),
                    KeyCode::Char('G') => state.jump_to_end(),
                    KeyCode::Char('r') => {
                        queue_refresh_for_active_tab(&mut state);
                    }
                    KeyCode::Char('c') if state.active_tab == AppTab::Leaderboard => {
                        cycle_leaderboard_category(&mut state.config);
                        match state.config.write_to_path(&state.config_path) {
                            Ok(()) => state.refresh_leaderboard = true,
                            Err(error) => {
                                state.status_message =
                                    format_status_error("config write failed", &error);
                            }
                        }
                    }
                    KeyCode::Char('t') if state.active_tab == AppTab::Leaderboard => {
                        cycle_leaderboard_time_period(&mut state.config);
                        match state.config.write_to_path(&state.config_path) {
                            Ok(()) => state.refresh_leaderboard = true,
                            Err(error) => {
                                state.status_message =
                                    format_status_error("config write failed", &error);
                            }
                        }
                    }
                    KeyCode::Char('o') if state.active_tab == AppTab::Leaderboard => {
                        cycle_leaderboard_ordering(&mut state.config);
                        match state.config.write_to_path(&state.config_path) {
                            Ok(()) => state.refresh_leaderboard = true,
                            Err(error) => {
                                state.status_message =
                                    format_status_error("config write failed", &error);
                            }
                        }
                    }
                    KeyCode::Char('i') => {
                        if let Some(context) = state.selected_context()
                            && let Err(error) = inspect_context(&client, &mut state, context).await
                        {
                            state.status_message =
                                format_status_error("wallet inspect failed", &error);
                        }
                    }
                    KeyCode::Char('a') if state.active_tab == AppTab::Leaderboard => {
                        if let Some(SelectionContext::Leaderboard(index)) = state.selected_context()
                        {
                            add_leaderboard_wallet_to_watchlist(&mut state, index, false)?;
                        }
                    }
                    KeyCode::Char('d') if state.active_tab == AppTab::Watchlist => {
                        if let Some(SelectionContext::Watchlist(index)) = state.selected_context() {
                            remove_watchlist_wallet(&mut state, index)?;
                        }
                    }
                    KeyCode::Char('p') => {
                        if let Some(context) = state.selected_context() {
                            toggle_selected_paper_follow(&mut state, context)?;
                        }
                    }
                    KeyCode::Enter => {
                        if let Some(context) = state.selected_context() {
                            state.action_menu = Some(build_action_menu(&state, context));
                        }
                    }
                    _ => {}
                }
            }
        }

        Ok::<(), anyhow::Error>(())
    }
    .await;

    if let Some(task) = watchlist_task {
        task.abort();
    }
    if let Some(task) = leaderboard_task {
        task.abort();
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    run_result
}

fn spawn_watchlist_refresh(
    client: PolymarketClient,
    state: &AppState,
) -> JoinHandle<Result<WatchlistRefreshResult>> {
    let config = state.config.clone();
    let previous_rows = state
        .watchlist_rows
        .iter()
        .map(|row| (row.config_index, row.clone()))
        .collect::<HashMap<_, _>>();

    tokio::spawn(async move { load_watchlist_refresh(client, config, previous_rows).await })
}

async fn load_watchlist_refresh(
    client: PolymarketClient,
    config: AppConfig,
    previous_rows: HashMap<usize, WatchedWalletRow>,
) -> Result<WatchlistRefreshResult> {
    let concurrency = config.http.max_concurrent_requests.max(1);
    let mut outcomes = stream::iter(config.monitor.wallets.iter().cloned().enumerate())
        .map(|(config_index, watched)| {
            let config = &config;
            let client = &client;
            async move {
                let result = build_resolved_wallet_analysis(client, config, &watched.wallet).await;
                WatchlistRefreshOutcome {
                    config_index,
                    watched,
                    result,
                }
            }
        })
        .buffer_unordered(concurrency)
        .collect::<Vec<_>>()
        .await;
    outcomes.sort_by_key(|outcome| outcome.config_index);

    let mut rows = Vec::with_capacity(outcomes.len());
    let mut stale_wallets = 0usize;
    let mut dropped_wallets = 0usize;

    for outcome in outcomes {
        let config_index = outcome.config_index;
        let watched = outcome.watched;
        let ResolvedWalletAnalysis { resolved, analysis } = match outcome.result {
            Ok(result) => result,
            Err(_) => {
                if let Some(previous) = previous_rows.get(&config_index) {
                    let mut stale_row = previous.clone();
                    stale_row.paper_follow_enabled = watched.paper_follow_enabled;
                    rows.push(stale_row);
                    stale_wallets += 1;
                    continue;
                }
                dropped_wallets += 1;
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
            &config,
            &label,
            &resolved.wallet,
            &analysis.report,
            &analysis.activities,
        )?;

        rows.push(WatchedWalletRow {
            config_index,
            wallet: resolved.wallet,
            label,
            paper_follow_enabled: watched.paper_follow_enabled,
            analysis,
        });
    }

    let paper_dashboard = build_paper_dashboard(&rows, &config)?;

    Ok(WatchlistRefreshResult {
        rows,
        paper_dashboard,
        refreshed_at: now_ts(),
        stale_wallets,
        dropped_wallets,
    })
}

fn apply_watchlist_refresh(state: &mut AppState, result: WatchlistRefreshResult) {
    state.watchlist_rows = result.rows;
    state.watchlist_selected = move_index(state.watchlist_selected, state.watchlist_rows.len(), 0);
    state.paper_dashboard = result.paper_dashboard;
    state.last_refresh_timestamp = Some(result.refreshed_at);
    state.status_message =
        watchlist_refresh_status(state, result.stale_wallets, result.dropped_wallets);

    if let Some(detail) = &mut state.wallet_detail
        && let Some(row) = state
            .watchlist_rows
            .iter()
            .find(|row| row.wallet == detail.wallet)
    {
        detail.label = row.label.clone();
        detail.watched = true;
        detail.paper_follow_enabled = row.paper_follow_enabled;
        detail.analysis = row.analysis.clone();
        detail.source = "Watchlist".to_owned();
    }
}

fn watchlist_refresh_status(
    state: &AppState,
    stale_wallets: usize,
    dropped_wallets: usize,
) -> String {
    if state.watchlist_rows.is_empty() {
        return "watchlist is empty".to_owned();
    }

    if !state
        .watchlist_rows
        .iter()
        .any(|row| row.paper_follow_enabled)
    {
        return format!(
            "watchlist refreshed: {} wallet(s) | paper follow disabled",
            state.watchlist_rows.len()
        );
    }

    let mut summary = format!(
        "watchlist refreshed: {} wallet(s) | paper {} | {} new trade(s)",
        state.watchlist_rows.len(),
        if state.paper_dashboard.resumed_journal {
            "resumed"
        } else {
            "rebuilt"
        },
        state.paper_dashboard.processed_activity_count
    );
    if stale_wallets > 0 {
        summary.push_str(&format!(" | stale {}", stale_wallets));
    }
    if dropped_wallets > 0 {
        summary.push_str(&format!(" | dropped {}", dropped_wallets));
    }
    summary
}

fn spawn_leaderboard_refresh(
    client: PolymarketClient,
    state: &AppState,
) -> JoinHandle<Result<LeaderboardRefreshResult>> {
    let config = state.config.clone();
    let watch_map = state
        .watchlist_rows
        .iter()
        .map(|row| (row.wallet.clone(), row.paper_follow_enabled))
        .collect::<HashMap<_, _>>();

    tokio::spawn(async move { load_leaderboard_refresh(client, config, watch_map).await })
}

async fn load_leaderboard_refresh(
    client: PolymarketClient,
    config: AppConfig,
    watch_map: HashMap<String, bool>,
) -> Result<LeaderboardRefreshResult> {
    let leaderboard = client
        .leaderboard(
            config.discover.category,
            config.discover.time_period,
            config.discover.order_by,
            config.discover.candidate_count,
            0,
        )
        .await?;

    let concurrency = config.http.max_concurrent_requests.max(1);
    let mut outcomes = stream::iter(leaderboard.into_iter().enumerate())
        .map(|(index, entry)| {
            let config = &config;
            let client = &client;
            async move {
                let wallet = entry.proxy_wallet.to_ascii_lowercase();
                let report =
                    build_wallet_report(client, config, &wallet, Some(entry.clone())).await?;
                Ok::<_, anyhow::Error>(LeaderboardRefreshOutcome {
                    index,
                    entry,
                    wallet,
                    report,
                })
            }
        })
        .buffer_unordered(concurrency)
        .try_collect::<Vec<_>>()
        .await?;
    outcomes.sort_by_key(|outcome| outcome.index);

    let mut rows = Vec::with_capacity(outcomes.len());
    for outcome in outcomes {
        let LeaderboardRefreshOutcome {
            entry,
            wallet,
            report,
            ..
        } = outcome;
        let label = entry
            .user_name
            .clone()
            .or(report.scorecard.user_name.clone())
            .unwrap_or_else(|| shorten_wallet(&wallet));
        let paper_follow_enabled = watch_map.get(&wallet).copied().unwrap_or(false);
        rows.push(LeaderboardRow {
            entry,
            wallet: wallet.clone(),
            label,
            report,
            watched: watch_map.contains_key(&wallet),
            paper_follow_enabled,
        });
    }

    Ok(LeaderboardRefreshResult {
        rows,
        refreshed_at: now_ts(),
    })
}

fn apply_leaderboard_refresh(state: &mut AppState, result: LeaderboardRefreshResult) {
    state.leaderboard_rows = result.rows;
    state.leaderboard_selected =
        move_index(state.leaderboard_selected, state.leaderboard_rows.len(), 0);
    state.last_refresh_timestamp = Some(result.refreshed_at);
    state.status_message = format!(
        "leaderboard refreshed: {} {:?} wallets",
        state.leaderboard_rows.len(),
        state.config.discover.category
    );
}

fn build_paper_dashboard(rows: &[WatchedWalletRow], config: &AppConfig) -> Result<PaperDashboard> {
    let progress = build_shared_paper_runtime(
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

    let mut wallets = rows
        .iter()
        .filter(|row| row.paper_follow_enabled)
        .map(|row| PaperWalletRow {
            label: row.label.clone(),
            wallet: row.wallet.clone(),
            recommendation: recommendation_label(
                row.analysis.report.scorecard.recommendation.clone(),
            ),
            total_pnl: row.analysis.report.simulation.total_pnl,
            final_equity: row.analysis.report.simulation.final_equity,
            followed_trades: row.analysis.report.simulation.followed_trades,
            closed_trades: row.analysis.report.simulation.closed_trades,
        })
        .collect::<Vec<_>>();
    wallets.sort_by(|left, right| right.total_pnl.total_cmp(&left.total_pnl));

    Ok(PaperDashboard {
        recent_executions: progress.recent_executions,
        summary: progress.summary,
        wallets,
        processed_activity_count: progress.processed_activity_count,
        resumed_journal: progress.resumed_journal,
    })
}

fn build_action_menu(state: &AppState, context: SelectionContext) -> ActionMenuState {
    let (watched, paper_follow_enabled) = match context {
        SelectionContext::Leaderboard(index) => state
            .leaderboard_rows
            .get(index)
            .map(|row| (row.watched, row.paper_follow_enabled))
            .unwrap_or((false, false)),
        SelectionContext::Watchlist(index) => state
            .watchlist_rows
            .get(index)
            .map(|row| (true, row.paper_follow_enabled))
            .unwrap_or((false, false)),
    };

    let mut actions = vec![WalletAction::InspectWallet];
    if watched {
        if paper_follow_enabled {
            actions.push(WalletAction::StopPaperFollow);
        } else {
            actions.push(WalletAction::StartPaperFollow);
        }
        actions.push(WalletAction::RemoveFromWatchlist);
    } else {
        actions.push(WalletAction::AddToWatchlist);
        actions.push(WalletAction::StartPaperFollow);
    }
    actions.push(WalletAction::RefreshData);

    ActionMenuState {
        context,
        selected: 0,
        actions,
    }
}

async fn perform_action(
    client: &PolymarketClient,
    state: &mut AppState,
    context: SelectionContext,
    action: WalletAction,
) -> Result<()> {
    match action {
        WalletAction::InspectWallet => inspect_context(client, state, context).await,
        WalletAction::AddToWatchlist => {
            if let SelectionContext::Leaderboard(index) = context {
                add_leaderboard_wallet_to_watchlist(state, index, false)?;
            }
            Ok(())
        }
        WalletAction::RemoveFromWatchlist => remove_context_from_watchlist(state, context),
        WalletAction::StartPaperFollow => set_context_paper_follow(state, context, true),
        WalletAction::StopPaperFollow => set_context_paper_follow(state, context, false),
        WalletAction::RefreshData => {
            queue_refresh_for_context(state, &context);
            Ok(())
        }
    }
}

async fn inspect_context(
    client: &PolymarketClient,
    state: &mut AppState,
    context: SelectionContext,
) -> Result<()> {
    match context {
        SelectionContext::Watchlist(index) => {
            let Some(row) = state.watchlist_rows.get(index) else {
                return Ok(());
            };
            state.wallet_detail = Some(WalletDetail {
                wallet: row.wallet.clone(),
                label: row.label.clone(),
                source: "Watchlist".to_owned(),
                watched: true,
                paper_follow_enabled: row.paper_follow_enabled,
                analysis: row.analysis.clone(),
            });
        }
        SelectionContext::Leaderboard(index) => {
            let Some(row) = state.leaderboard_rows.get(index) else {
                return Ok(());
            };
            let analysis =
                build_wallet_analysis(client, &state.config, &row.wallet, Some(row.entry.clone()))
                    .await?;
            state.wallet_detail = Some(WalletDetail {
                wallet: row.wallet.clone(),
                label: row.label.clone(),
                source: "Leaderboard".to_owned(),
                watched: row.watched,
                paper_follow_enabled: row.paper_follow_enabled,
                analysis,
            });
        }
    }

    state.active_tab = AppTab::Wallet;
    state.status_message = "wallet inspection ready".to_owned();
    Ok(())
}

fn add_leaderboard_wallet_to_watchlist(
    state: &mut AppState,
    index: usize,
    paper_follow_enabled: bool,
) -> Result<()> {
    let Some(row) = state.leaderboard_rows.get(index) else {
        return Ok(());
    };

    if state
        .watchlist_rows
        .iter()
        .any(|watched| watched.wallet == row.wallet)
    {
        state.status_message = "wallet is already in watchlist".to_owned();
        return Ok(());
    }

    state.config.monitor.wallets.push(WatchedWalletConfig {
        wallet: row.wallet.clone(),
        label: Some(row.label.clone()),
        paper_follow_enabled,
    });
    state.config.write_to_path(&state.config_path)?;
    state.status_message = format!("added {} to watchlist", row.label);
    state.refresh_watchlist = true;
    state.refresh_leaderboard = true;
    Ok(())
}

fn remove_watchlist_wallet(state: &mut AppState, index: usize) -> Result<()> {
    let Some(row) = state.watchlist_rows.get(index) else {
        return Ok(());
    };
    if row.config_index < state.config.monitor.wallets.len() {
        state.config.monitor.wallets.remove(row.config_index);
        state.config.write_to_path(&state.config_path)?;
        state.status_message = format!("removed {} from watchlist", row.label);
        state.watchlist_selected = state.watchlist_selected.saturating_sub(1);
        state.refresh_watchlist = true;
        state.refresh_leaderboard = true;
    }
    Ok(())
}

fn remove_context_from_watchlist(state: &mut AppState, context: SelectionContext) -> Result<()> {
    match context {
        SelectionContext::Watchlist(index) => remove_watchlist_wallet(state, index),
        SelectionContext::Leaderboard(index) => {
            let Some(row) = state.leaderboard_rows.get(index) else {
                return Ok(());
            };
            let Some(existing) = state
                .watchlist_rows
                .iter()
                .find(|watched| watched.wallet == row.wallet)
                .cloned()
            else {
                return Ok(());
            };
            remove_watchlist_wallet(
                state,
                state
                    .watchlist_rows
                    .iter()
                    .position(|watched| watched.wallet == existing.wallet)
                    .unwrap_or_default(),
            )
        }
    }
}

fn set_context_paper_follow(
    state: &mut AppState,
    context: SelectionContext,
    enabled: bool,
) -> Result<()> {
    let wallet = match context {
        SelectionContext::Leaderboard(index) => {
            let Some(row) = state.leaderboard_rows.get(index) else {
                return Ok(());
            };
            row.wallet.clone()
        }
        SelectionContext::Watchlist(index) => {
            let Some(row) = state.watchlist_rows.get(index) else {
                return Ok(());
            };
            row.wallet.clone()
        }
    };

    if let Some(index) = state
        .watchlist_rows
        .iter()
        .find(|row| row.wallet == wallet)
        .map(|row| row.config_index)
    {
        if let Some(watched) = state.config.monitor.wallets.get_mut(index) {
            watched.paper_follow_enabled = enabled;
        }
    } else if let Some(row) = state
        .leaderboard_rows
        .iter()
        .find(|row| row.wallet == wallet)
        .cloned()
    {
        state.config.monitor.wallets.push(WatchedWalletConfig {
            wallet: row.wallet.clone(),
            label: Some(row.label.clone()),
            paper_follow_enabled: enabled,
        });
    }

    state.config.write_to_path(&state.config_path)?;
    state.status_message = if enabled {
        format!("paper-follow enabled for {}", wallet)
    } else {
        format!("paper-follow disabled for {}", wallet)
    };
    state.refresh_watchlist = true;
    state.refresh_leaderboard = true;
    Ok(())
}

fn toggle_selected_paper_follow(state: &mut AppState, context: SelectionContext) -> Result<()> {
    let enabled = match context {
        SelectionContext::Leaderboard(index) => state
            .leaderboard_rows
            .get(index)
            .map(|row| !row.paper_follow_enabled)
            .unwrap_or(false),
        SelectionContext::Watchlist(index) => state
            .watchlist_rows
            .get(index)
            .map(|row| !row.paper_follow_enabled)
            .unwrap_or(false),
    };
    set_context_paper_follow(state, context, enabled)
}

fn queue_refresh_for_active_tab(state: &mut AppState) {
    match state.active_tab {
        AppTab::Leaderboard => state.refresh_leaderboard = true,
        AppTab::Watchlist | AppTab::Wallet | AppTab::Paper => {
            state.refresh_watchlist = true;
            if matches!(state.active_tab, AppTab::Wallet) {
                state.refresh_leaderboard = true;
            }
        }
    }
}

fn queue_refresh_for_context(state: &mut AppState, context: &SelectionContext) {
    match context {
        SelectionContext::Leaderboard(_) => state.refresh_leaderboard = true,
        SelectionContext::Watchlist(_) => state.refresh_watchlist = true,
    }
}

fn cycle_leaderboard_category(config: &mut AppConfig) {
    config.discover.category =
        cycle_enum(config.discover.category, &LEADERBOARD_CATEGORIES).unwrap_or_default();
}

fn cycle_leaderboard_time_period(config: &mut AppConfig) {
    config.discover.time_period =
        cycle_enum(config.discover.time_period, &LEADERBOARD_TIME_PERIODS).unwrap_or_default();
}

fn cycle_leaderboard_ordering(config: &mut AppConfig) {
    config.discover.order_by =
        cycle_enum(config.discover.order_by, &LEADERBOARD_ORDERINGS).unwrap_or_default();
}

fn draw_app(frame: &mut Frame, state: &AppState) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(2),
        ])
        .split(frame.area());

    let titles = AppTab::ALL
        .iter()
        .map(|tab| Line::from(tab.title()))
        .collect::<Vec<_>>();
    let selected_tab = AppTab::ALL
        .iter()
        .position(|tab| *tab == state.active_tab)
        .unwrap_or_default();
    let tabs = Tabs::new(titles)
        .select(selected_tab)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Mochi Gain Hunter"),
        )
        .style(Style::default().fg(Color::White))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );

    frame.render_widget(tabs, layout[0]);

    match state.active_tab {
        AppTab::Leaderboard => draw_leaderboard_tab(frame, layout[1], state),
        AppTab::Watchlist => draw_watchlist_tab(frame, layout[1], state),
        AppTab::Wallet => draw_wallet_tab(frame, layout[1], state),
        AppTab::Paper => draw_paper_tab(frame, layout[1], state),
    }

    let footer = Paragraph::new(footer_text(state))
        .block(Block::default().borders(Borders::ALL))
        .wrap(Wrap { trim: true });
    frame.render_widget(footer, layout[2]);

    if let Some(menu) = &state.action_menu {
        draw_action_menu(frame, menu);
    }
}

fn draw_leaderboard_tab(frame: &mut Frame, area: Rect, state: &AppState) {
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(area);
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(9), Constraint::Min(8)])
        .split(body[1]);

    let filters = Paragraph::new(vec![
        Line::from(vec![
            Span::raw("category "),
            Span::styled(
                format!("{:?}", state.config.discover.category).to_uppercase(),
                Style::default().fg(Color::Yellow),
            ),
            Span::raw("  time "),
            Span::styled(
                format!("{:?}", state.config.discover.time_period).to_uppercase(),
                Style::default().fg(Color::Yellow),
            ),
            Span::raw("  order "),
            Span::styled(
                format!("{:?}", state.config.discover.order_by).to_uppercase(),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(format!(
            "limit {} | c cycle category | t cycle period | o cycle order | enter actions",
            state.config.discover.candidate_count
        )),
    ])
    .block(Block::default().title("Filters").borders(Borders::ALL))
    .wrap(Wrap { trim: true });

    render_stateful_list(
        frame,
        body[0],
        leaderboard_list(state),
        state.leaderboard_selected,
        state.leaderboard_rows.len(),
    );
    frame.render_widget(filters, right[0]);
    frame.render_widget(draw_leaderboard_summary(state), right[1]);
}

fn draw_watchlist_tab(frame: &mut Frame, area: Rect, state: &AppState) {
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(10), Constraint::Min(8)])
        .split(body[1]);

    render_stateful_list(
        frame,
        body[0],
        watchlist_list(state),
        state.watchlist_selected,
        state.watchlist_rows.len(),
    );
    frame.render_widget(draw_watchlist_summary(state), right[0]);
    frame.render_widget(draw_watchlist_recent_trades(state), right[1]);
}

fn draw_wallet_tab(frame: &mut Frame, area: Rect, state: &AppState) {
    let Some(detail) = &state.wallet_detail else {
        let empty = Paragraph::new(
            "Inspect a leaderboard or watchlist wallet with Enter or i to load the wallet view.",
        )
        .block(Block::default().title("Wallet").borders(Borders::ALL))
        .wrap(Wrap { trim: true });
        frame.render_widget(empty, area);
        return;
    };

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(10),
            Constraint::Percentage(55),
            Constraint::Percentage(45),
        ])
        .split(area);
    let lower = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(layout[1]);

    frame.render_widget(draw_wallet_detail_summary(detail), layout[0]);
    frame.render_widget(draw_wallet_activity_history(detail), lower[0]);
    frame.render_widget(draw_wallet_positions(detail), lower[1]);
    frame.render_widget(draw_wallet_closed_trades(detail), layout[2]);
}

fn draw_paper_tab(frame: &mut Frame, area: Rect, state: &AppState) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(10), Constraint::Min(10)])
        .split(area);
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(34), Constraint::Percentage(66)])
        .split(layout[1]);
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
        .split(body[1]);

    frame.render_widget(draw_paper_summary(state), layout[0]);
    frame.render_widget(draw_paper_wallets(state), body[0]);
    frame.render_widget(draw_paper_positions(state), right[0]);
    frame.render_widget(draw_paper_executions(state), right[1]);
}

fn leaderboard_list(state: &AppState) -> List<'static> {
    let items = if state.leaderboard_rows.is_empty() {
        vec![ListItem::new("No leaderboard candidates loaded.")]
    } else {
        state
            .leaderboard_rows
            .iter()
            .map(|row| {
                let pnl_style = if row.report.simulation.total_pnl >= 0.0 {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::Red)
                };
                let watch_style = if row.paper_follow_enabled {
                    Style::default().fg(Color::Green)
                } else if row.watched {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                ListItem::new(vec![
                    Line::from(vec![
                        Span::styled(
                            format!("#{} ", row.entry.rank),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::styled(
                            truncate_text(&row.label, 24),
                            Style::default().add_modifier(Modifier::BOLD),
                        ),
                    ]),
                    Line::from(vec![
                        Span::raw(format!(
                            "score {:.1} | reco {} | ",
                            row.report.scorecard.score,
                            recommendation_label(row.report.scorecard.recommendation.clone())
                        )),
                        Span::styled(
                            if row.paper_follow_enabled {
                                "paper on"
                            } else if row.watched {
                                "watch only"
                            } else {
                                "not watched"
                            },
                            watch_style,
                        ),
                    ]),
                    Line::from(vec![
                        Span::raw("sim "),
                        Span::styled(format!("{:.2}", row.report.simulation.total_pnl), pnl_style),
                        Span::raw(format!(" | vol {:.0}", row.entry.volume)),
                    ]),
                ])
            })
            .collect()
    };

    List::new(items)
        .block(
            Block::default()
                .title("Leaderboard Candidates")
                .borders(Borders::ALL),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ")
}

fn watchlist_list(state: &AppState) -> List<'static> {
    let items = if state.watchlist_rows.is_empty() {
        vec![ListItem::new("No watched wallets yet.")]
    } else {
        state
            .watchlist_rows
            .iter()
            .map(|row| {
                let pnl_style = if row.analysis.report.simulation.total_pnl >= 0.0 {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::Red)
                };
                ListItem::new(vec![
                    Line::from(vec![
                        Span::styled(
                            truncate_text(&row.label, 22),
                            Style::default().add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(" "),
                        Span::styled(
                            if row.paper_follow_enabled {
                                "[paper]"
                            } else {
                                "[watch]"
                            },
                            if row.paper_follow_enabled {
                                Style::default().fg(Color::Green)
                            } else {
                                Style::default().fg(Color::Yellow)
                            },
                        ),
                    ]),
                    Line::from(format!(
                        "score {:.1} | reco {} | recent {}",
                        row.analysis.report.scorecard.score,
                        recommendation_label(row.analysis.report.scorecard.recommendation.clone()),
                        row.analysis.report.scorecard.aggregates.recent_trade_count
                    )),
                    Line::from(vec![
                        Span::raw("sim "),
                        Span::styled(
                            format!("{:.2}", row.analysis.report.simulation.total_pnl),
                            pnl_style,
                        ),
                        Span::raw(format!(
                            " | open {}",
                            row.analysis.report.simulation.open_positions.len()
                        )),
                    ]),
                ])
            })
            .collect()
    };

    List::new(items)
        .block(
            Block::default()
                .title("Watched Wallets")
                .borders(Borders::ALL),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ")
}

fn render_stateful_list(
    frame: &mut Frame,
    area: Rect,
    list: List<'static>,
    selected: usize,
    len: usize,
) {
    let mut list_state = ListState::default();
    if len > 0 {
        list_state.select(Some(selected));
    }
    frame.render_stateful_widget(list, area, &mut list_state);
}

fn draw_leaderboard_summary(state: &AppState) -> Paragraph<'static> {
    let Some(row) = state.leaderboard_rows.get(state.leaderboard_selected) else {
        return empty_block("Selected Candidate", "No candidate selected.");
    };

    let score_style = Style::default()
        .fg(score_color(row.report.scorecard.score))
        .add_modifier(Modifier::BOLD);
    let pnl_style = pnl_style(row.report.simulation.total_pnl);
    let gate_text = if row.report.scorecard.gating_reasons.is_empty() {
        "ok".to_owned()
    } else {
        row.report.scorecard.gating_reasons.join(" | ")
    };

    Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                truncate_text(&row.label, 36),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                format!("score {:.2}", row.report.scorecard.score),
                score_style,
            ),
        ]),
        Line::from(format!(
            "rank #{} | pnl {:.2} | vol {:.2}",
            row.entry.rank, row.entry.pnl, row.entry.volume
        )),
        Line::from(format!(
            "reco {} | avg trade ${:.2} | win rate {:.1}%",
            recommendation_label(row.report.scorecard.recommendation.clone()),
            row.report.scorecard.aggregates.average_trade_usdc,
            row.report.scorecard.aggregates.win_rate * 100.0
        )),
        Line::from(vec![
            Span::raw("paper sim "),
            Span::styled(format!("{:.2}", row.report.simulation.total_pnl), pnl_style),
            Span::raw(format!(
                " | followed {}",
                row.report.simulation.followed_trades
            )),
        ]),
        Line::from(format!(
            "watched {} | paper {}",
            yes_no(row.watched),
            yes_no(row.paper_follow_enabled)
        )),
        Line::from(format!("wallet {}", row.wallet)),
        Line::from(format!("status {}", gate_text)),
    ])
    .block(
        Block::default()
            .title("Selected Candidate")
            .borders(Borders::ALL),
    )
    .wrap(Wrap { trim: true })
}

fn draw_watchlist_summary(state: &AppState) -> Paragraph<'static> {
    let Some(row) = state.watchlist_rows.get(state.watchlist_selected) else {
        return empty_block("Selected Wallet", "No watched wallet selected.");
    };

    let report = &row.analysis.report;
    let gate_text = if report.scorecard.gating_reasons.is_empty() {
        "ok".to_owned()
    } else {
        report.scorecard.gating_reasons.join(" | ")
    };

    Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                truncate_text(&row.label, 34),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                format!("score {:.2}", report.scorecard.score),
                Style::default()
                    .fg(score_color(report.scorecard.score))
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(format!(
            "wallet {} | paper {} | reco {}",
            row.wallet,
            yes_no(row.paper_follow_enabled),
            recommendation_label(report.scorecard.recommendation.clone())
        )),
        Line::from(format!(
            "recent {} | open {} | last trade {}",
            report.scorecard.aggregates.recent_trade_count,
            report.scorecard.aggregates.open_position_count,
            opt_timestamp(report.scorecard.aggregates.last_trade_timestamp)
        )),
        Line::from(format!(
            "realized {:.2} | open {:.2} | concentration {:.2}",
            report.scorecard.aggregates.realized_pnl_total,
            report.scorecard.aggregates.open_pnl_total,
            report.scorecard.aggregates.top_position_ratio
        )),
        Line::from(vec![
            Span::raw("paper sim "),
            Span::styled(
                format!("{:.2}", report.simulation.total_pnl),
                pnl_style(report.simulation.total_pnl),
            ),
            Span::raw(format!(" | equity {:.2}", report.simulation.final_equity)),
        ]),
        Line::from(format!("status {}", gate_text)),
    ])
    .block(
        Block::default()
            .title("Selected Wallet")
            .borders(Borders::ALL),
    )
    .wrap(Wrap { trim: true })
}

fn draw_watchlist_recent_trades(state: &AppState) -> Paragraph<'static> {
    let Some(row) = state.watchlist_rows.get(state.watchlist_selected) else {
        return empty_block("Recent Trades", "No watchlist activity loaded.");
    };
    Paragraph::new(wallet_trade_lines(&row.label, &row.analysis.activities, 18))
        .block(
            Block::default()
                .title("Recent Trades")
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: true })
}

fn draw_wallet_detail_summary(detail: &WalletDetail) -> Paragraph<'static> {
    let report = &detail.analysis.report;
    let gate_text = if report.scorecard.gating_reasons.is_empty() {
        "ok".to_owned()
    } else {
        report.scorecard.gating_reasons.join(" | ")
    };
    Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                truncate_text(&detail.label, 34),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                format!("score {:.2}", report.scorecard.score),
                Style::default()
                    .fg(score_color(report.scorecard.score))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                recommendation_label(report.scorecard.recommendation.clone()),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(format!(
            "source {} | watched {} | paper {}",
            detail.source,
            yes_no(detail.watched),
            yes_no(detail.paper_follow_enabled)
        )),
        Line::from(format!("wallet {}", detail.wallet)),
        Line::from(format!(
            "avg trade ${:.2} | win rate {:.1}% | recent {}",
            report.scorecard.aggregates.average_trade_usdc,
            report.scorecard.aggregates.win_rate * 100.0,
            report.scorecard.aggregates.recent_trade_count
        )),
        Line::from(vec![
            Span::raw("paper sim "),
            Span::styled(
                format!("{:.2}", report.simulation.total_pnl),
                pnl_style(report.simulation.total_pnl),
            ),
            Span::raw(format!(
                " | followed {} | open {}",
                report.simulation.followed_trades,
                report.simulation.open_positions.len()
            )),
        ]),
        Line::from(format!("status {}", gate_text)),
    ])
    .block(
        Block::default()
            .title("Wallet Overview")
            .borders(Borders::ALL),
    )
    .wrap(Wrap { trim: true })
}

fn draw_wallet_activity_history(detail: &WalletDetail) -> Paragraph<'static> {
    Paragraph::new(wallet_trade_lines(
        &detail.label,
        &detail.analysis.activities,
        16,
    ))
    .block(
        Block::default()
            .title("Trade History")
            .borders(Borders::ALL),
    )
    .wrap(Wrap { trim: true })
}

fn draw_wallet_positions(detail: &WalletDetail) -> Paragraph<'static> {
    let positions = &detail.analysis.report.simulation.open_positions;
    let lines = if positions.is_empty() {
        vec![Line::from("No open paper positions.")]
    } else {
        positions
            .iter()
            .take(14)
            .map(|position| {
                Line::from(vec![
                    Span::raw(format!("{:.2} ", position.size)),
                    Span::raw(format!(
                        "@ {:.4} -> {:.4} ",
                        position.avg_entry_price, position.mark_price
                    )),
                    Span::styled(
                        format!("pnl {:.2} ", position.unrealized_pnl),
                        pnl_style(position.unrealized_pnl),
                    ),
                    Span::raw(truncate_text(
                        position.title.as_deref().unwrap_or(position.asset.as_str()),
                        28,
                    )),
                ])
            })
            .collect::<Vec<_>>()
    };

    Paragraph::new(lines)
        .block(
            Block::default()
                .title("Open Paper Positions")
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: true })
}

fn draw_wallet_closed_trades(detail: &WalletDetail) -> Paragraph<'static> {
    let trades = &detail.analysis.report.simulation.closed_positions;
    let lines = if trades.is_empty() {
        vec![Line::from("No closed paper trades yet.")]
    } else {
        trades
            .iter()
            .rev()
            .take(14)
            .map(|trade| {
                Line::from(vec![
                    Span::styled(format!("pnl {:.2} ", trade.pnl), pnl_style(trade.pnl)),
                    Span::raw(format!(
                        "{:.2} @ {:.4}->{:.4} ",
                        trade.size, trade.entry_price, trade.exit_price
                    )),
                    Span::raw(truncate_text(
                        trade.title.as_deref().unwrap_or(trade.asset.as_str()),
                        46,
                    )),
                ])
            })
            .collect::<Vec<_>>()
    };

    Paragraph::new(lines)
        .block(
            Block::default()
                .title("Closed Paper Trades")
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: true })
}

fn draw_paper_summary(state: &AppState) -> Paragraph<'static> {
    let summary = &state.paper_dashboard.summary;
    let top_reason = summary
        .skip_reasons
        .first()
        .map(|reason| format!("{} x{}", reason.reason, reason.count))
        .unwrap_or_else(|| "none".to_owned());
    let journal_mode = if state.paper_dashboard.resumed_journal {
        "resumed"
    } else {
        "rebuilt"
    };
    Paragraph::new(vec![
        Line::from(format!(
            "shared bankroll {:.2} | tracked wallets {}",
            summary.starting_cash, summary.tracked_wallets
        )),
        Line::from(format!(
            "followed {} | ignored {} | closed {}",
            summary.followed_trades, summary.ignored_trades, summary.closed_trades
        )),
        Line::from(format!(
            "cash {:.2} | equity {:.2}",
            summary.final_cash, summary.final_equity
        )),
        Line::from(format!(
            "deployed {:.2} | marked {:.2}",
            summary.deployed_cost_basis, summary.deployed_market_value
        )),
        Line::from(vec![
            Span::raw("total pnl "),
            Span::styled(
                format!("{:.2}", summary.total_pnl),
                pnl_style(summary.total_pnl),
            ),
            Span::raw(format!(" | reserve {:.2}", summary.cash_reserve_target)),
        ]),
        Line::from(format!(
            "realized {:.2} | unrealized {:.2}",
            summary.realized_pnl, summary.unrealized_pnl
        )),
        Line::from(format!(
            "open {} | friction {}",
            summary.open_positions.len(),
            truncate_text(&top_reason, 48)
        )),
        Line::from(format!(
            "journal {} | new activity {}",
            journal_mode, state.paper_dashboard.processed_activity_count
        )),
        Line::from(format!(
            "tracked {} -> {}",
            opt_timestamp(summary.tracked_from_timestamp),
            opt_timestamp(summary.tracked_to_timestamp)
        )),
    ])
    .block(
        Block::default()
            .title("Shared Paper Account")
            .borders(Borders::ALL),
    )
    .wrap(Wrap { trim: true })
}

fn draw_paper_wallets(state: &AppState) -> Paragraph<'static> {
    let lines = if state.paper_dashboard.wallets.is_empty() {
        vec![Line::from(
            "Enable paper-follow on at least one watchlist wallet.",
        )]
    } else {
        state
            .paper_dashboard
            .wallets
            .iter()
            .map(|wallet| {
                Line::from(vec![
                    Span::styled(
                        truncate_text(&wallet.label, 20),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        wallet.recommendation.clone(),
                        Style::default().fg(Color::Yellow),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        format!("{:.2}", wallet.total_pnl),
                        pnl_style(wallet.total_pnl),
                    ),
                    Span::raw(format!(
                        " | eq {:.2} | {}/{} | {}",
                        wallet.final_equity,
                        wallet.followed_trades,
                        wallet.closed_trades,
                        shorten_wallet(&wallet.wallet)
                    )),
                ])
            })
            .collect::<Vec<_>>()
    };

    Paragraph::new(lines)
        .block(
            Block::default()
                .title("Enabled Wallets")
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: true })
}

fn draw_paper_positions(state: &AppState) -> Paragraph<'static> {
    let positions = &state.paper_dashboard.summary.open_positions;
    let lines = if positions.is_empty() {
        vec![Line::from("No open positions in the shared paper account.")]
    } else {
        positions
            .iter()
            .take(16)
            .map(format_portfolio_position)
            .collect::<Vec<_>>()
    };

    Paragraph::new(lines)
        .block(
            Block::default()
                .title("Open Shared Positions")
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: true })
}

fn draw_paper_executions(state: &AppState) -> Paragraph<'static> {
    let executions = &state.paper_dashboard.recent_executions;
    let lines = if executions.is_empty() {
        vec![Line::from("No paper-account decisions yet.")]
    } else {
        executions
            .iter()
            .take(18)
            .map(format_execution_line)
            .collect::<Vec<_>>()
    };

    Paragraph::new(lines)
        .block(
            Block::default()
                .title("Recent Paper Decisions")
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: true })
}

fn draw_action_menu(frame: &mut Frame, menu: &ActionMenuState) {
    let area = centered_rect(frame.area(), 50, 40);
    let items = menu
        .actions
        .iter()
        .map(|action| ListItem::new(action.label()))
        .collect::<Vec<_>>();
    let list = List::new(items)
        .block(
            Block::default()
                .title("Wallet Actions")
                .borders(Borders::ALL),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    let mut state = ListState::default();
    state.select(Some(menu.selected));
    frame.render_widget(Clear, area);
    frame.render_stateful_widget(list, area, &mut state);
}

fn footer_text(state: &AppState) -> String {
    let base = format!(
        "tabs h/l or tab | move j/k | enter actions | i inspect | p paper toggle | r refresh | last refresh {} | {}",
        opt_timestamp(state.last_refresh_timestamp),
        state.status_message,
    );

    match state.active_tab {
        AppTab::Leaderboard => format!("{base} | c category | t time | o order | a add"),
        AppTab::Watchlist => format!("{base} | d remove"),
        AppTab::Wallet | AppTab::Paper => base,
    }
}

fn wallet_trade_lines(
    label: &str,
    activities: &[WalletActivity],
    limit: usize,
) -> Vec<Line<'static>> {
    let mut trades = activities
        .iter()
        .filter(|activity| matches!(activity.activity_type, WalletActivityType::Trade))
        .filter(|activity| matches!(activity.side, Some(TradeSide::Buy | TradeSide::Sell)))
        .collect::<Vec<_>>();
    trades.sort_by(|left, right| right.timestamp.cmp(&left.timestamp));

    if trades.is_empty() {
        return vec![Line::from("No recent trades captured.")];
    }

    trades
        .into_iter()
        .take(limit)
        .map(|activity| {
            let side = activity.side.unwrap_or(TradeSide::Unknown);
            Line::from(vec![
                Span::styled(
                    format!("[{}] ", activity.timestamp),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw(format!("{label} ")),
                Span::styled(
                    format!("{:?} ", side).to_uppercase(),
                    if side == TradeSide::Buy {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::Red)
                    },
                ),
                Span::raw(format!(
                    "${:.2} @ {:.4} ",
                    activity.usdc_size, activity.price
                )),
                Span::raw(truncate_text(
                    activity
                        .title
                        .as_deref()
                        .or(activity.slug.as_deref())
                        .unwrap_or(activity.asset.as_str()),
                    54,
                )),
            ])
        })
        .collect()
}

fn format_portfolio_position(position: &PortfolioSimulationPosition) -> Line<'static> {
    let label = position
        .source_label
        .clone()
        .unwrap_or_else(|| shorten_wallet(&position.source_wallet));
    let market_value = position.size * position.mark_price;
    Line::from(vec![
        Span::styled(label, Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" "),
        Span::raw(format!("{:.2} ", position.size)),
        Span::raw(format!("val {:.2} ", market_value)),
        Span::styled(
            format!("pnl {:.2} ", position.unrealized_pnl),
            pnl_style(position.unrealized_pnl),
        ),
        Span::raw(truncate_text(
            position.title.as_deref().unwrap_or(position.asset.as_str()),
            40,
        )),
    ])
}

fn format_execution_line(execution: &PortfolioSimulationExecution) -> Line<'static> {
    let label = execution
        .source_label
        .clone()
        .unwrap_or_else(|| shorten_wallet(&execution.source_wallet));
    let status_style = match execution.status {
        SimulationExecutionStatus::Filled => Style::default().fg(Color::Green),
        SimulationExecutionStatus::Partial => Style::default().fg(Color::Yellow),
        SimulationExecutionStatus::Skipped | SimulationExecutionStatus::Canceled => {
            Style::default().fg(Color::Red)
        }
    };
    Line::from(vec![
        Span::styled(
            format!("[{}] ", execution.timestamp),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(label, Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" "),
        Span::styled(
            format!("{:?} ", execution.side).to_uppercase(),
            if execution.side == TradeSide::Buy {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Red)
            },
        ),
        Span::styled(
            format!("{:?} ", execution.status).to_uppercase(),
            status_style,
        ),
        Span::raw(format!(
            "{:.2}/{:.2} @ {:.4} ",
            execution.filled_usdc, execution.requested_usdc, execution.price
        )),
        Span::raw(truncate_text(
            execution.reason.as_deref().unwrap_or_else(|| {
                execution
                    .title
                    .as_deref()
                    .unwrap_or(execution.asset.as_str())
            }),
            36,
        )),
    ])
}

fn empty_block<'a>(title: &'a str, message: &'a str) -> Paragraph<'static> {
    Paragraph::new(message.to_owned())
        .block(
            Block::default()
                .title(title.to_owned())
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: true })
}

fn move_index(current: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    let len = len as isize;
    let current = current as isize;
    (current + delta).clamp(0, len - 1) as usize
}

fn cycle_enum<T>(current: T, items: &[T]) -> Option<T>
where
    T: Copy + PartialEq,
{
    let index = items.iter().position(|item| *item == current)?;
    Some(items[(index + 1) % items.len()])
}

fn centered_rect(area: Rect, width_percent: u16, height_percent: u16) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - height_percent) / 2),
            Constraint::Percentage(height_percent),
            Constraint::Percentage((100 - height_percent) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - width_percent) / 2),
            Constraint::Percentage(width_percent),
            Constraint::Percentage((100 - width_percent) / 2),
        ])
        .flex(Flex::Center)
        .split(vertical[1])[1]
}

fn recommendation_label(recommendation: FollowRecommendation) -> String {
    match recommendation {
        FollowRecommendation::Ignore => "IGNORE".to_owned(),
        FollowRecommendation::Watch => "WATCH".to_owned(),
        FollowRecommendation::ManualReview => "MANUAL_REVIEW".to_owned(),
        FollowRecommendation::PaperFollow => "PAPER_FOLLOW".to_owned(),
    }
}

fn score_color(score: f64) -> Color {
    if score >= 80.0 {
        Color::Green
    } else if score >= 65.0 {
        Color::Yellow
    } else {
        Color::Red
    }
}

fn pnl_style(value: f64) -> Style {
    if value >= 0.0 {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::Red)
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn opt_timestamp(value: Option<i64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "pending".to_owned())
}

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn shorten_wallet(wallet: &str) -> String {
    let prefix = &wallet[..6.min(wallet.len())];
    let suffix = &wallet[wallet.len().saturating_sub(4)..];
    format!("{prefix}...{suffix}")
}

fn truncate_text(value: &str, max_chars: usize) -> String {
    let mut truncated = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        truncated.push_str("...");
    }
    truncated
}

fn format_status_error(prefix: &str, error: &anyhow::Error) -> String {
    let message = error
        .chain()
        .map(|part| part.to_string())
        .collect::<Vec<_>>()
        .join(" | ");
    format!("{}: {}", prefix, truncate_text(&message, 120))
}
