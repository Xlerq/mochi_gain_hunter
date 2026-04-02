use std::collections::{HashSet, VecDeque};
use std::io;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Result, bail};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use serde::Serialize;
use tokio::time::sleep;

use crate::config::{AppConfig, WatchedWalletConfig};
use crate::domain::{
    FollowRecommendation, TradeSide, WalletActivity, WalletActivityType, WalletReport,
};
use crate::polymarket::PolymarketClient;
use crate::reporting::build_wallet_analysis;
use crate::storage::persist_wallet_tracking;

#[derive(Debug, Clone)]
struct ResolvedWatchTarget {
    wallet: String,
    label: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct WalletMonitorRow {
    label: String,
    wallet: String,
    score: f64,
    recommendation: String,
    eligible: bool,
    recent_trades: usize,
    open_positions: usize,
    last_trade_timestamp: Option<i64>,
    sim_total_pnl: f64,
    status: String,
    average_trade_usdc: f64,
    win_rate: f64,
    realized_pnl_total: f64,
    open_pnl_total: f64,
    top_position_ratio: f64,
    tracked_trade_count: usize,
    paper_followed_trades: usize,
    paper_closed_trades: usize,
    paper_final_cash: f64,
    paper_final_equity: f64,
}

#[derive(Debug, Clone, Serialize)]
struct RecentTradeEvent {
    label: String,
    wallet: String,
    timestamp: i64,
    title: String,
    side: String,
    price: f64,
    usdc_size: f64,
    slug: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct PaperBookSummary {
    wallet_count: usize,
    total_followed_trades: usize,
    total_closed_trades: usize,
    total_open_positions: usize,
    total_realized_pnl: f64,
    total_unrealized_pnl: f64,
    total_pnl: f64,
    total_final_cash: f64,
    total_final_equity: f64,
    total_starting_cash: f64,
}

#[derive(Debug, Clone, Serialize)]
struct MonitorSnapshot {
    bankroll: f64,
    poll_interval_secs: u64,
    last_refresh_timestamp: Option<i64>,
    paper_book: PaperBookSummary,
    wallets: Vec<WalletMonitorRow>,
    recent_trades: Vec<RecentTradeEvent>,
}

struct MonitorState {
    snapshot: MonitorSnapshot,
    seen_activity_ids: HashSet<String>,
    recent_trades: VecDeque<RecentTradeEvent>,
    selected_wallet: usize,
    status_message: String,
}

impl MonitorState {
    fn new(config: &AppConfig) -> Self {
        Self {
            snapshot: MonitorSnapshot {
                bankroll: config.simulation.starting_cash,
                poll_interval_secs: config.monitor.poll_interval_secs,
                last_refresh_timestamp: None,
                paper_book: PaperBookSummary {
                    wallet_count: 0,
                    total_followed_trades: 0,
                    total_closed_trades: 0,
                    total_open_positions: 0,
                    total_realized_pnl: 0.0,
                    total_unrealized_pnl: 0.0,
                    total_pnl: 0.0,
                    total_final_cash: 0.0,
                    total_final_equity: 0.0,
                    total_starting_cash: 0.0,
                },
                wallets: Vec::new(),
                recent_trades: Vec::new(),
            },
            seen_activity_ids: HashSet::new(),
            recent_trades: VecDeque::new(),
            selected_wallet: 0,
            status_message: "starting monitor".to_owned(),
        }
    }
}

pub async fn run_monitor(
    config_path: &Path,
    wallet_override: Option<&str>,
    plain: bool,
    cycles: Option<usize>,
) -> Result<()> {
    let config = AppConfig::load_or_default(config_path)?;
    let client = PolymarketClient::new(&config)?;
    let watchlist = resolve_watchlist(&client, &config, wallet_override).await?;
    let mut state = MonitorState::new(&config);

    if plain {
        run_plain_monitor(
            &client,
            &config,
            &watchlist,
            cycles.unwrap_or(1),
            &mut state,
        )
        .await?;
        return Ok(());
    }

    run_tui_monitor(&client, &config, &watchlist, cycles, &mut state).await
}

async fn run_plain_monitor(
    client: &PolymarketClient,
    config: &AppConfig,
    watchlist: &[ResolvedWatchTarget],
    cycles: usize,
    state: &mut MonitorState,
) -> Result<()> {
    for cycle in 0..cycles.max(1) {
        refresh_state(client, config, watchlist, state).await?;
        println!("{}", serde_json::to_string_pretty(&state.snapshot)?);

        if cycle + 1 < cycles {
            sleep(Duration::from_secs(config.monitor.poll_interval_secs)).await;
        }
    }

    Ok(())
}

async fn run_tui_monitor(
    client: &PolymarketClient,
    config: &AppConfig,
    watchlist: &[ResolvedWatchTarget],
    cycles: Option<usize>,
    state: &mut MonitorState,
) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut refresh_deadline = Instant::now();
    let mut completed_cycles = 0usize;

    let run_result = async {
        loop {
            if completed_cycles == 0
                || refresh_deadline.elapsed()
                    >= Duration::from_secs(config.monitor.poll_interval_secs)
            {
                state.status_message = "refreshing live wallet data".to_owned();
                refresh_state(client, config, watchlist, state).await?;
                completed_cycles += 1;
                refresh_deadline = Instant::now();
                state.status_message = "live monitor ready".to_owned();
            }

            terminal.draw(|frame| draw_dashboard(frame, state))?;

            if let Some(max_cycles) = cycles {
                if completed_cycles >= max_cycles {
                    break;
                }
            }

            if event::poll(Duration::from_millis(250))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        match key.code {
                            KeyCode::Char('q') => break,
                            KeyCode::Char('r') => {
                                refresh_deadline = Instant::now()
                                    - Duration::from_secs(config.monitor.poll_interval_secs);
                            }
                            KeyCode::Up | KeyCode::Char('k') => move_selection(state, -1),
                            KeyCode::Down | KeyCode::Char('j') => move_selection(state, 1),
                            KeyCode::Char('g') => state.selected_wallet = 0,
                            KeyCode::Char('G') => {
                                state.selected_wallet =
                                    state.snapshot.wallets.len().saturating_sub(1);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        Ok::<(), anyhow::Error>(())
    }
    .await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    run_result
}

async fn refresh_state(
    client: &PolymarketClient,
    config: &AppConfig,
    watchlist: &[ResolvedWatchTarget],
    state: &mut MonitorState,
) -> Result<()> {
    let mut rows = Vec::with_capacity(watchlist.len());
    let mut paper_book = PaperBookSummary {
        wallet_count: watchlist.len(),
        total_followed_trades: 0,
        total_closed_trades: 0,
        total_open_positions: 0,
        total_realized_pnl: 0.0,
        total_unrealized_pnl: 0.0,
        total_pnl: 0.0,
        total_final_cash: 0.0,
        total_final_equity: 0.0,
        total_starting_cash: 0.0,
    };

    for watched in watchlist {
        let analysis = build_wallet_analysis(client, config, &watched.wallet, None).await?;
        let report = analysis.report;
        let label = watched
            .label
            .clone()
            .or_else(|| report.scorecard.user_name.clone())
            .unwrap_or_else(|| shorten_wallet(&watched.wallet));

        let tracked_trade_count = record_recent_trades(
            &mut state.seen_activity_ids,
            &mut state.recent_trades,
            &label,
            &watched.wallet,
            &analysis.activities,
            config.monitor.recent_events_limit,
        );

        persist_wallet_tracking(
            config,
            &label,
            &watched.wallet,
            &report,
            &analysis.activities,
        )?;
        rows.push(wallet_row_from_report(
            &label,
            &watched.wallet,
            &report,
            tracked_trade_count,
        ));

        paper_book.total_followed_trades += report.simulation.followed_trades;
        paper_book.total_closed_trades += report.simulation.closed_trades;
        paper_book.total_open_positions += report.simulation.open_positions.len();
        paper_book.total_realized_pnl += report.simulation.realized_pnl;
        paper_book.total_unrealized_pnl += report.simulation.unrealized_pnl;
        paper_book.total_pnl += report.simulation.total_pnl;
        paper_book.total_final_cash += report.simulation.final_cash;
        paper_book.total_final_equity += report.simulation.final_equity;
        paper_book.total_starting_cash += report.simulation.starting_cash;
    }

    state.snapshot.last_refresh_timestamp = Some(now_ts());
    state.snapshot.paper_book = paper_book;
    state.snapshot.wallets = rows;
    sort_recent_trades(&mut state.recent_trades);
    state.snapshot.recent_trades = state.recent_trades.iter().cloned().collect();
    if state.selected_wallet >= state.snapshot.wallets.len() {
        state.selected_wallet = state.snapshot.wallets.len().saturating_sub(1);
    }

    Ok(())
}

async fn resolve_watchlist(
    client: &PolymarketClient,
    config: &AppConfig,
    wallet_override: Option<&str>,
) -> Result<Vec<ResolvedWatchTarget>> {
    if let Some(wallet_override) = wallet_override {
        let resolved = client.resolve_wallet_input(wallet_override).await?;
        return Ok(vec![ResolvedWatchTarget {
            wallet: resolved.wallet,
            label: resolved.label.or(resolved.username),
        }]);
    }

    let mut watchlist = Vec::new();
    for watched in &config.monitor.wallets {
        watchlist.push(resolve_watched_target(client, watched).await?);
    }

    if watchlist.is_empty() {
        bail!("no wallets configured for monitoring");
    }

    Ok(watchlist)
}

async fn resolve_watched_target(
    client: &PolymarketClient,
    watched: &WatchedWalletConfig,
) -> Result<ResolvedWatchTarget> {
    let resolved = client.resolve_wallet_input(&watched.wallet).await?;
    Ok(ResolvedWatchTarget {
        wallet: resolved.wallet,
        label: watched
            .label
            .clone()
            .or(resolved.label)
            .or(resolved.username),
    })
}

fn wallet_row_from_report(
    label: &str,
    wallet: &str,
    report: &WalletReport,
    tracked_trade_count: usize,
) -> WalletMonitorRow {
    WalletMonitorRow {
        label: label.to_owned(),
        wallet: wallet.to_owned(),
        score: report.scorecard.score,
        recommendation: recommendation_label(report.scorecard.recommendation.clone()),
        eligible: report.scorecard.eligible,
        recent_trades: report.scorecard.aggregates.recent_trade_count,
        open_positions: report.scorecard.aggregates.open_position_count,
        last_trade_timestamp: report.scorecard.aggregates.last_trade_timestamp,
        sim_total_pnl: report.simulation.total_pnl,
        status: if report.scorecard.gating_reasons.is_empty() {
            "ok".to_owned()
        } else {
            report.scorecard.gating_reasons.join(" | ")
        },
        average_trade_usdc: report.scorecard.aggregates.average_trade_usdc,
        win_rate: report.scorecard.aggregates.win_rate,
        realized_pnl_total: report.scorecard.aggregates.realized_pnl_total,
        open_pnl_total: report.scorecard.aggregates.open_pnl_total,
        top_position_ratio: report.scorecard.aggregates.top_position_ratio,
        tracked_trade_count,
        paper_followed_trades: report.simulation.followed_trades,
        paper_closed_trades: report.simulation.closed_trades,
        paper_final_cash: report.simulation.final_cash,
        paper_final_equity: report.simulation.final_equity,
    }
}

fn record_recent_trades(
    seen_activity_ids: &mut HashSet<String>,
    recent_trades: &mut VecDeque<RecentTradeEvent>,
    label: &str,
    wallet: &str,
    activities: &[WalletActivity],
    recent_limit: usize,
) -> usize {
    let mut recent_matches = activities
        .iter()
        .filter(|activity| matches!(activity.activity_type, WalletActivityType::Trade))
        .filter(|activity| matches!(activity.side, Some(TradeSide::Buy | TradeSide::Sell)))
        .collect::<Vec<_>>();

    recent_matches.sort_by_key(|activity| activity.timestamp);

    for activity in &recent_matches {
        let activity_id = activity_id(wallet, activity);
        if seen_activity_ids.insert(activity_id) {
            recent_trades.push_front(RecentTradeEvent {
                label: label.to_owned(),
                wallet: wallet.to_owned(),
                timestamp: activity.timestamp,
                title: activity
                    .title
                    .clone()
                    .or_else(|| activity.slug.clone())
                    .unwrap_or_else(|| activity.asset.clone()),
                side: side_label(activity.side).to_owned(),
                price: activity.price,
                usdc_size: activity.usdc_size,
                slug: activity.slug.clone(),
            });
        }
    }

    while recent_trades.len() > recent_limit {
        recent_trades.pop_back();
    }

    recent_matches.len()
}

fn activity_id(wallet: &str, activity: &WalletActivity) -> String {
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

fn recommendation_label(recommendation: FollowRecommendation) -> String {
    match recommendation {
        FollowRecommendation::Ignore => "IGNORE".to_owned(),
        FollowRecommendation::Watch => "WATCH".to_owned(),
        FollowRecommendation::ManualReview => "MANUAL_REVIEW".to_owned(),
        FollowRecommendation::PaperFollow => "PAPER_FOLLOW".to_owned(),
    }
}

fn shorten_wallet(wallet: &str) -> String {
    let prefix = &wallet[..6.min(wallet.len())];
    let suffix_start = wallet.len().saturating_sub(4);
    let suffix = &wallet[suffix_start..];
    format!("{prefix}...{suffix}")
}

fn move_selection(state: &mut MonitorState, delta: isize) {
    if state.snapshot.wallets.is_empty() {
        state.selected_wallet = 0;
        return;
    }

    let len = state.snapshot.wallets.len() as isize;
    let current = state.selected_wallet as isize;
    let next = (current + delta).clamp(0, len - 1);
    state.selected_wallet = next as usize;
}

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn sort_recent_trades(recent_trades: &mut VecDeque<RecentTradeEvent>) {
    let mut items = recent_trades.drain(..).collect::<Vec<_>>();
    items.sort_by(|left, right| right.timestamp.cmp(&left.timestamp));
    *recent_trades = items.into();
}

fn draw_dashboard(frame: &mut Frame, state: &MonitorState) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(10), Constraint::Length(2)])
        .split(frame.area());

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(33), Constraint::Percentage(67)])
        .split(layout[0]);

    let selected_wallet = state.snapshot.wallets.get(state.selected_wallet);
    let wallet_list = draw_wallet_list(state);
    let detail_area = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(11),
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(body[1]);
    let summary_row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(56), Constraint::Percentage(44)])
        .split(detail_area[0]);

    let selected_summary = draw_selected_wallet_summary(selected_wallet);
    let paper_book = draw_paper_book_summary(&state.snapshot.paper_book);
    let selected_trades =
        draw_selected_trades(state, selected_wallet.map(|wallet| wallet.wallet.as_str()));
    let global_trades = draw_global_trades(state);

    let footer = Paragraph::new(format!(
        "q quit | r refresh | j/k or arrows move | g/G first/last | per-wallet bankroll ${:.2} | poll {}s | last refresh {} | {}",
        state.snapshot.bankroll,
        state.snapshot.poll_interval_secs,
        state
            .snapshot
            .last_refresh_timestamp
            .map(|value| value.to_string())
            .unwrap_or_else(|| "pending".to_owned()),
        state.status_message,
    ))
        .block(Block::default().borders(Borders::ALL));

    render_wallet_list(
        frame,
        body[0],
        wallet_list,
        state.selected_wallet,
        state.snapshot.wallets.len(),
    );
    frame.render_widget(selected_summary, summary_row[0]);
    frame.render_widget(paper_book, summary_row[1]);
    frame.render_widget(selected_trades, detail_area[1]);
    frame.render_widget(global_trades, detail_area[2]);
    frame.render_widget(footer, layout[1]);
}

fn draw_wallet_list(state: &MonitorState) -> List<'static> {
    let items = if state.snapshot.wallets.is_empty() {
        vec![ListItem::new("No wallets loaded.")]
    } else {
        state
            .snapshot
            .wallets
            .iter()
            .map(|wallet| {
                let pnl_style = if wallet.sim_total_pnl >= 0.0 {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::Red)
                };

                ListItem::new(vec![
                    Line::from(vec![
                        Span::styled(
                            wallet.label.clone(),
                            Style::default().add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(" "),
                        Span::styled(
                            format!("{:.1}", wallet.score),
                            Style::default().fg(score_color(wallet.score)),
                        ),
                    ]),
                    Line::from(vec![
                        Span::raw(format!(
                            "{} | recent {} | open {} | ",
                            wallet.recommendation, wallet.recent_trades, wallet.open_positions
                        )),
                        Span::styled(format!("PnL {:.2}", wallet.sim_total_pnl), pnl_style),
                    ]),
                ])
            })
            .collect::<Vec<_>>()
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

fn render_wallet_list(
    frame: &mut Frame,
    area: Rect,
    list: List<'static>,
    selected_wallet: usize,
    wallet_count: usize,
) {
    let mut list_state = ListState::default();
    if wallet_count > 0 {
        list_state.select(Some(selected_wallet));
    }
    frame.render_stateful_widget(list, area, &mut list_state);
}

fn draw_selected_wallet_summary(selected_wallet: Option<&WalletMonitorRow>) -> Paragraph<'static> {
    let Some(wallet) = selected_wallet else {
        return Paragraph::new("No wallet selected.").block(
            Block::default()
                .title("Selected Wallet")
                .borders(Borders::ALL),
        );
    };

    let score_style = Style::default()
        .fg(score_color(wallet.score))
        .add_modifier(Modifier::BOLD);
    let pnl_style = if wallet.sim_total_pnl >= 0.0 {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::Red)
    };

    let text = vec![
        Line::from(vec![
            Span::styled(
                wallet.label.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(format!("score {:.2}", wallet.score), score_style),
            Span::raw("  "),
            Span::styled(
                if wallet.eligible {
                    "eligible"
                } else {
                    "not eligible"
                },
                if wallet.eligible {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::Yellow)
                },
            ),
        ]),
        Line::from(format!("wallet: {}", wallet.wallet)),
        Line::from(vec![Span::raw(format!(
            "reco {} | avg trade ${:.2} | win rate {:.1}% | tracked trades {}",
            wallet.recommendation,
            wallet.average_trade_usdc,
            wallet.win_rate * 100.0,
            wallet.tracked_trade_count
        ))]),
        Line::from(vec![Span::raw(format!(
            "realized {:.2} | open {:.2} | concentration {:.2}",
            wallet.realized_pnl_total, wallet.open_pnl_total, wallet.top_position_ratio
        ))]),
        Line::from(vec![Span::raw(format!(
            "paper trades {} / {} | cash {:.2} | equity {:.2}",
            wallet.paper_followed_trades,
            wallet.paper_closed_trades,
            wallet.paper_final_cash,
            wallet.paper_final_equity
        ))]),
        Line::from(vec![
            Span::raw("paper-follow pnl "),
            Span::styled(format!("{:.2}", wallet.sim_total_pnl), pnl_style),
            Span::raw(" | "),
            Span::raw(wallet.status.clone()),
        ]),
    ];

    Paragraph::new(text)
        .block(
            Block::default()
                .title("Selected Wallet")
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: true })
}

fn draw_paper_book_summary(paper_book: &PaperBookSummary) -> Paragraph<'static> {
    let pnl_style = if paper_book.total_pnl >= 0.0 {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    };

    Paragraph::new(vec![
        Line::from(format!(
            "wallets {} | open {}",
            paper_book.wallet_count, paper_book.total_open_positions
        )),
        Line::from(format!(
            "paper trades {} / {}",
            paper_book.total_followed_trades, paper_book.total_closed_trades
        )),
        Line::from(format!(
            "cash {:.2} | equity {:.2}",
            paper_book.total_final_cash, paper_book.total_final_equity
        )),
        Line::from(format!(
            "realized {:.2} | unrealized {:.2}",
            paper_book.total_realized_pnl, paper_book.total_unrealized_pnl
        )),
        Line::from(vec![
            Span::raw("total pnl "),
            Span::styled(format!("{:.2}", paper_book.total_pnl), pnl_style),
            Span::raw(format!(" | start {:.2}", paper_book.total_starting_cash)),
        ]),
    ])
    .block(Block::default().title("Paper Book").borders(Borders::ALL))
    .wrap(Wrap { trim: true })
}

fn draw_selected_trades(state: &MonitorState, wallet: Option<&str>) -> Paragraph<'static> {
    let lines = filtered_trade_lines(state, wallet, 12);
    Paragraph::new(lines)
        .block(
            Block::default()
                .title("Selected Wallet Recent Trades")
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: true })
}

fn draw_global_trades(state: &MonitorState) -> Paragraph<'static> {
    let lines = filtered_trade_lines(state, None, 12);
    Paragraph::new(lines)
        .block(
            Block::default()
                .title("Global Recent Trades")
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: true })
}

fn filtered_trade_lines(
    state: &MonitorState,
    wallet: Option<&str>,
    limit: usize,
) -> Vec<Line<'static>> {
    let trades = state
        .snapshot
        .recent_trades
        .iter()
        .filter(|trade| {
            wallet
                .map(|selected| trade.wallet == selected)
                .unwrap_or(true)
        })
        .take(limit)
        .collect::<Vec<_>>();

    if trades.is_empty() {
        return vec![Line::from("No recent trades captured yet.")];
    }

    trades
        .into_iter()
        .map(|trade| {
            let side_style = if trade.side == "BUY" {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Red)
            };

            Line::from(vec![
                Span::styled(
                    format!("[{}] ", trade.timestamp),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw(format!("{} ", trade.label)),
                Span::styled(format!("{} ", trade.side), side_style),
                Span::raw(format!("${:.2} @ {:.4} ", trade.usdc_size, trade.price)),
                Span::raw(truncate_text(&trade.title, 68)),
            ])
        })
        .collect()
}

fn truncate_text(value: &str, max_chars: usize) -> String {
    let mut truncated = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        truncated.push_str("...");
    }
    truncated
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
