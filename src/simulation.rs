use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::config::SimulationConfig;
use crate::domain::{
    ClosedSimulationTrade, OpenSimulationPosition, PortfolioSimulationExecution,
    PortfolioSimulationPosition, PortfolioSimulationReport, SimulationExecutionStatus,
    SimulationReport, SimulationSkipReasonCount, TradeSide, WalletActivity, WalletActivityType,
};

const EPSILON: f64 = 0.000_001;
const RECENT_EXECUTION_LIMIT: usize = 18;
const FORWARD_PAPER_JOURNAL_VERSION: u32 = 1;

#[derive(Debug, Clone)]
struct PositionState {
    source_wallet: String,
    source_label: Option<String>,
    asset: String,
    title: Option<String>,
    leader_size: f64,
    follower_size: f64,
    avg_entry_price: f64,
    last_mark_price: f64,
    last_buy_timestamp: i64,
}

#[derive(Debug, Clone)]
pub struct SharedSimulationInput {
    pub source_wallet: String,
    pub source_label: Option<String>,
    pub activities: Vec<WalletActivity>,
    pub current_marks: HashMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ForwardPaperJournalMetadata {
    pub enabled_wallets: Vec<String>,
    pub simulation_config: SimulationConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForwardPaperJournalPosition {
    pub source_wallet: String,
    pub source_label: Option<String>,
    pub asset: String,
    pub title: Option<String>,
    pub leader_size: f64,
    pub follower_size: f64,
    pub avg_entry_price: f64,
    pub last_mark_price: f64,
    pub last_buy_timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForwardPaperJournalPendingOrder {
    pub order_id: u64,
    pub source_wallet: String,
    pub source_label: Option<String>,
    pub asset: String,
    pub title: Option<String>,
    pub side: TradeSide,
    pub leader_timestamp: i64,
    pub scheduled_timestamp: i64,
    pub reference_price: f64,
    pub requested_usdc: f64,
    pub initial_requested_usdc: f64,
    pub leader_size_remaining: f64,
    pub initial_leader_size: f64,
    pub was_reduced_before_fill: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForwardPaperJournalBufferedBuy {
    pub source_wallet: String,
    pub source_label: Option<String>,
    pub asset: String,
    pub title: Option<String>,
    pub first_timestamp: i64,
    pub last_timestamp: i64,
    pub weighted_price_numerator: f64,
    pub buffered_requested_usdc: f64,
    pub buffered_leader_size: f64,
    pub buffered_leader_usdc: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForwardPaperJournalState {
    pub version: u32,
    pub metadata: ForwardPaperJournalMetadata,
    pub cash: f64,
    pub followed_trades: usize,
    pub ignored_trades: usize,
    pub closed_trades: usize,
    pub realized_pnl: f64,
    pub tracked_from_timestamp: Option<i64>,
    pub tracked_to_timestamp: Option<i64>,
    pub processed_activity_ids: Vec<String>,
    pub open_positions: Vec<ForwardPaperJournalPosition>,
    pub pending_orders: Vec<ForwardPaperJournalPendingOrder>,
    pub buffered_buys: Vec<ForwardPaperJournalBufferedBuy>,
    pub recent_executions: Vec<PortfolioSimulationExecution>,
    pub skip_reasons: Vec<SimulationSkipReasonCount>,
}

#[derive(Debug, Clone)]
pub struct ForwardPaperJournalProgress {
    pub state: ForwardPaperJournalState,
    pub report: PortfolioSimulationReport,
    pub new_executions: Vec<PortfolioSimulationExecution>,
    pub processed_activity_count: usize,
    pub resumed: bool,
}

pub fn can_resume_forward_paper_journal(
    journal: Option<&ForwardPaperJournalState>,
    metadata: &ForwardPaperJournalMetadata,
) -> bool {
    journal.is_some_and(|journal| {
        journal.version == FORWARD_PAPER_JOURNAL_VERSION && journal.metadata == *metadata
    })
}

#[derive(Debug, Clone)]
struct PendingOrder {
    order_id: u64,
    source_wallet: String,
    source_label: Option<String>,
    asset: String,
    title: Option<String>,
    side: TradeSide,
    leader_timestamp: i64,
    scheduled_timestamp: i64,
    reference_price: f64,
    requested_usdc: f64,
    initial_requested_usdc: f64,
    leader_size_remaining: f64,
    initial_leader_size: f64,
    was_reduced_before_fill: bool,
}

#[derive(Debug, Clone)]
struct BufferedBuySignal {
    source_wallet: String,
    source_label: Option<String>,
    asset: String,
    title: Option<String>,
    first_timestamp: i64,
    last_timestamp: i64,
    weighted_price_numerator: f64,
    buffered_requested_usdc: f64,
    buffered_leader_size: f64,
    buffered_leader_usdc: f64,
}

struct EngineState {
    cash: f64,
    followed_trades: usize,
    ignored_trades: usize,
    closed_trades: usize,
    realized_pnl: f64,
    open_positions: HashMap<(String, String), PositionState>,
    closed_positions: Vec<ClosedSimulationTrade>,
    executions: Vec<PortfolioSimulationExecution>,
    skip_reasons: HashMap<String, usize>,
    pending_orders: Vec<PendingOrder>,
    buffered_buys: HashMap<(String, String), BufferedBuySignal>,
    processed_activity_ids: HashSet<String>,
    next_order_id: u64,
    tracked_from_timestamp: Option<i64>,
    tracked_to_timestamp: Option<i64>,
}

pub fn simulate_copy_trading(
    wallet: &str,
    activities: &[WalletActivity],
    current_marks: &HashMap<String, f64>,
    config: &SimulationConfig,
) -> SimulationReport {
    let portfolio = simulate_shared_copy_trading(
        &[SharedSimulationInput {
            source_wallet: wallet.to_owned(),
            source_label: None,
            activities: activities.to_owned(),
            current_marks: current_marks.clone(),
        }],
        config,
    );

    let closed_trades = portfolio.closed_positions.len();
    let wins = portfolio
        .closed_positions
        .iter()
        .filter(|trade| trade.pnl > 0.0)
        .count();
    let win_rate = if closed_trades == 0 {
        0.0
    } else {
        wins as f64 / closed_trades as f64
    };

    let open_positions = portfolio
        .open_positions
        .iter()
        .filter(|position| position.source_wallet == wallet)
        .map(|position| OpenSimulationPosition {
            source_wallet: None,
            source_label: None,
            asset: position.asset.clone(),
            title: position.title.clone(),
            size: position.size,
            avg_entry_price: position.avg_entry_price,
            mark_price: position.mark_price,
            unrealized_pnl: position.unrealized_pnl,
        })
        .collect::<Vec<_>>();

    let closed_positions = portfolio
        .closed_positions
        .iter()
        .filter(|trade| trade.source_wallet.as_deref() == Some(wallet))
        .map(|trade| ClosedSimulationTrade {
            source_wallet: None,
            source_label: None,
            asset: trade.asset.clone(),
            title: trade.title.clone(),
            buy_timestamp: trade.buy_timestamp,
            sell_timestamp: trade.sell_timestamp,
            entry_price: trade.entry_price,
            exit_price: trade.exit_price,
            size: trade.size,
            pnl: trade.pnl,
        })
        .collect::<Vec<_>>();

    SimulationReport {
        wallet: wallet.to_owned(),
        followed_trades: portfolio.followed_trades,
        ignored_trades: portfolio.ignored_trades,
        closed_trades,
        win_rate,
        realized_pnl: portfolio.realized_pnl,
        unrealized_pnl: portfolio.unrealized_pnl,
        total_pnl: portfolio.total_pnl,
        final_cash: portfolio.final_cash,
        final_equity: portfolio.final_equity,
        starting_cash: portfolio.starting_cash,
        tracked_from_timestamp: portfolio.tracked_from_timestamp,
        tracked_to_timestamp: portfolio.tracked_to_timestamp,
        deployed_cost_basis: portfolio.deployed_cost_basis,
        deployed_market_value: portfolio.deployed_market_value,
        cash_reserve_target: portfolio.cash_reserve_target,
        skip_reasons: portfolio.skip_reasons.clone(),
        open_positions,
        closed_positions,
    }
}

pub fn simulate_shared_copy_trading(
    inputs: &[SharedSimulationInput],
    config: &SimulationConfig,
) -> PortfolioSimulationReport {
    let mut current_marks = HashMap::new();
    for input in inputs {
        for (asset, price) in &input.current_marks {
            current_marks.entry(asset.clone()).or_insert(*price);
        }
    }

    let mut trade_activities = inputs
        .iter()
        .flat_map(|input| {
            input.activities.iter().filter_map(move |activity| {
                if !matches!(activity.activity_type, WalletActivityType::Trade) {
                    return None;
                }
                if !matches!(activity.side, Some(TradeSide::Buy | TradeSide::Sell)) {
                    return None;
                }
                if activity.price <= 0.0 || activity.size <= 0.0 {
                    return None;
                }
                Some((
                    input.source_wallet.clone(),
                    input.source_label.clone(),
                    activity.clone(),
                ))
            })
        })
        .collect::<Vec<_>>();

    trade_activities.sort_by(|left, right| {
        left.2
            .timestamp
            .cmp(&right.2.timestamp)
            .then_with(|| left.0.cmp(&right.0))
            .then_with(|| left.2.asset.cmp(&right.2.asset))
    });

    let mut state = EngineState {
        cash: config.starting_cash,
        followed_trades: 0,
        ignored_trades: 0,
        closed_trades: 0,
        realized_pnl: 0.0,
        open_positions: HashMap::new(),
        closed_positions: Vec::new(),
        executions: Vec::new(),
        skip_reasons: HashMap::new(),
        pending_orders: Vec::new(),
        buffered_buys: HashMap::new(),
        processed_activity_ids: HashSet::new(),
        next_order_id: 0,
        tracked_from_timestamp: trade_activities
            .first()
            .map(|(_, _, activity)| activity.timestamp),
        tracked_to_timestamp: trade_activities
            .last()
            .map(|(_, _, activity)| activity.timestamp),
    };

    for (source_wallet, source_label, activity) in trade_activities {
        state
            .processed_activity_ids
            .insert(paper_activity_key(&source_wallet, &activity));
        process_due_orders(activity.timestamp, &current_marks, config, &mut state);
        handle_signal(&source_wallet, source_label, &activity, config, &mut state);
    }

    process_due_orders(i64::MAX, &current_marks, config, &mut state);
    finalize_buffered_buys(&mut state);
    build_portfolio_report(inputs.len(), &current_marks, config, &state)
}

pub fn advance_forward_paper_journal(
    previous: Option<ForwardPaperJournalState>,
    inputs: &[SharedSimulationInput],
    metadata: ForwardPaperJournalMetadata,
    config: &SimulationConfig,
    as_of_timestamp: i64,
) -> ForwardPaperJournalProgress {
    let mut current_marks = HashMap::new();
    for input in inputs {
        for (asset, price) in &input.current_marks {
            current_marks.entry(asset.clone()).or_insert(*price);
        }
    }

    let mut trade_activities = inputs
        .iter()
        .flat_map(|input| {
            input.activities.iter().filter_map(move |activity| {
                if !matches!(activity.activity_type, WalletActivityType::Trade) {
                    return None;
                }
                if !matches!(activity.side, Some(TradeSide::Buy | TradeSide::Sell)) {
                    return None;
                }
                if activity.price <= 0.0 || activity.size <= 0.0 {
                    return None;
                }
                Some((
                    input.source_wallet.clone(),
                    input.source_label.clone(),
                    activity.clone(),
                ))
            })
        })
        .collect::<Vec<_>>();

    trade_activities.sort_by(|left, right| {
        left.2
            .timestamp
            .cmp(&right.2.timestamp)
            .then_with(|| left.0.cmp(&right.0))
            .then_with(|| left.2.asset.cmp(&right.2.asset))
    });

    let resumable = previous.as_ref().is_some_and(|journal| {
        journal.version == FORWARD_PAPER_JOURNAL_VERSION && journal.metadata == metadata
    });

    let mut state = if resumable {
        engine_state_from_journal(previous.expect("checked above"))
    } else {
        new_engine_state(config.starting_cash)
    };

    let starting_execution_len = state.executions.len();
    let mut processed_activity_count = 0usize;

    for (source_wallet, source_label, activity) in trade_activities {
        let activity_id = paper_activity_key(&source_wallet, &activity);
        if !state.processed_activity_ids.insert(activity_id) {
            continue;
        }

        processed_activity_count += 1;
        state.tracked_from_timestamp = Some(
            state
                .tracked_from_timestamp
                .map(|timestamp| timestamp.min(activity.timestamp))
                .unwrap_or(activity.timestamp),
        );
        state.tracked_to_timestamp = Some(
            state
                .tracked_to_timestamp
                .map(|timestamp| timestamp.max(activity.timestamp))
                .unwrap_or(activity.timestamp),
        );

        process_due_orders(activity.timestamp, &current_marks, config, &mut state);
        handle_signal(&source_wallet, source_label, &activity, config, &mut state);
    }

    process_due_orders(as_of_timestamp, &current_marks, config, &mut state);

    let report = build_portfolio_report(inputs.len(), &current_marks, config, &state);
    let new_executions = state
        .executions
        .iter()
        .skip(starting_execution_len)
        .cloned()
        .collect::<Vec<_>>();
    let journal_state = journal_state_from_engine(&state, metadata, &report);

    ForwardPaperJournalProgress {
        state: journal_state,
        report,
        new_executions,
        processed_activity_count,
        resumed: resumable,
    }
}

fn new_engine_state(starting_cash: f64) -> EngineState {
    EngineState {
        cash: starting_cash,
        followed_trades: 0,
        ignored_trades: 0,
        closed_trades: 0,
        realized_pnl: 0.0,
        open_positions: HashMap::new(),
        closed_positions: Vec::new(),
        executions: Vec::new(),
        skip_reasons: HashMap::new(),
        pending_orders: Vec::new(),
        buffered_buys: HashMap::new(),
        processed_activity_ids: HashSet::new(),
        next_order_id: 0,
        tracked_from_timestamp: None,
        tracked_to_timestamp: None,
    }
}

fn engine_state_from_journal(journal: ForwardPaperJournalState) -> EngineState {
    let pending_orders = journal
        .pending_orders
        .into_iter()
        .map(|order| PendingOrder {
            order_id: order.order_id,
            source_wallet: order.source_wallet,
            source_label: order.source_label,
            asset: order.asset,
            title: order.title,
            side: order.side,
            leader_timestamp: order.leader_timestamp,
            scheduled_timestamp: order.scheduled_timestamp,
            reference_price: order.reference_price,
            requested_usdc: order.requested_usdc,
            initial_requested_usdc: order.initial_requested_usdc,
            leader_size_remaining: order.leader_size_remaining,
            initial_leader_size: order.initial_leader_size,
            was_reduced_before_fill: order.was_reduced_before_fill,
        })
        .collect::<Vec<_>>();
    let buffered_buys = journal
        .buffered_buys
        .into_iter()
        .map(|signal| {
            (
                (signal.source_wallet.clone(), signal.asset.clone()),
                BufferedBuySignal {
                    source_wallet: signal.source_wallet,
                    source_label: signal.source_label,
                    asset: signal.asset,
                    title: signal.title,
                    first_timestamp: signal.first_timestamp,
                    last_timestamp: signal.last_timestamp,
                    weighted_price_numerator: signal.weighted_price_numerator,
                    buffered_requested_usdc: signal.buffered_requested_usdc,
                    buffered_leader_size: signal.buffered_leader_size,
                    buffered_leader_usdc: signal.buffered_leader_usdc,
                },
            )
        })
        .collect::<HashMap<_, _>>();
    let open_positions = journal
        .open_positions
        .into_iter()
        .map(|position| {
            (
                (position.source_wallet.clone(), position.asset.clone()),
                PositionState {
                    source_wallet: position.source_wallet,
                    source_label: position.source_label,
                    asset: position.asset,
                    title: position.title,
                    leader_size: position.leader_size,
                    follower_size: position.follower_size,
                    avg_entry_price: position.avg_entry_price,
                    last_mark_price: position.last_mark_price,
                    last_buy_timestamp: position.last_buy_timestamp,
                },
            )
        })
        .collect::<HashMap<_, _>>();
    let skip_reasons = journal
        .skip_reasons
        .into_iter()
        .map(|reason| (reason.reason, reason.count))
        .collect::<HashMap<_, _>>();
    let next_order_id = pending_orders
        .iter()
        .map(|order| order.order_id)
        .max()
        .unwrap_or_default();

    EngineState {
        cash: journal.cash,
        followed_trades: journal.followed_trades,
        ignored_trades: journal.ignored_trades,
        closed_trades: journal.closed_trades,
        realized_pnl: journal.realized_pnl,
        open_positions,
        closed_positions: Vec::new(),
        executions: journal.recent_executions,
        skip_reasons,
        pending_orders,
        buffered_buys,
        processed_activity_ids: journal.processed_activity_ids.into_iter().collect(),
        next_order_id,
        tracked_from_timestamp: journal.tracked_from_timestamp,
        tracked_to_timestamp: journal.tracked_to_timestamp,
    }
}

fn journal_state_from_engine(
    state: &EngineState,
    metadata: ForwardPaperJournalMetadata,
    report: &PortfolioSimulationReport,
) -> ForwardPaperJournalState {
    let mut processed_activity_ids = state
        .processed_activity_ids
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    processed_activity_ids.sort();

    let mut open_positions = state
        .open_positions
        .values()
        .map(|position| ForwardPaperJournalPosition {
            source_wallet: position.source_wallet.clone(),
            source_label: position.source_label.clone(),
            asset: position.asset.clone(),
            title: position.title.clone(),
            leader_size: position.leader_size,
            follower_size: position.follower_size,
            avg_entry_price: position.avg_entry_price,
            last_mark_price: position.last_mark_price,
            last_buy_timestamp: position.last_buy_timestamp,
        })
        .collect::<Vec<_>>();
    open_positions.sort_by(|left, right| {
        left.source_wallet
            .cmp(&right.source_wallet)
            .then_with(|| left.asset.cmp(&right.asset))
    });

    let mut pending_orders = state
        .pending_orders
        .iter()
        .map(|order| ForwardPaperJournalPendingOrder {
            order_id: order.order_id,
            source_wallet: order.source_wallet.clone(),
            source_label: order.source_label.clone(),
            asset: order.asset.clone(),
            title: order.title.clone(),
            side: order.side,
            leader_timestamp: order.leader_timestamp,
            scheduled_timestamp: order.scheduled_timestamp,
            reference_price: order.reference_price,
            requested_usdc: order.requested_usdc,
            initial_requested_usdc: order.initial_requested_usdc,
            leader_size_remaining: order.leader_size_remaining,
            initial_leader_size: order.initial_leader_size,
            was_reduced_before_fill: order.was_reduced_before_fill,
        })
        .collect::<Vec<_>>();
    pending_orders.sort_by(|left, right| {
        left.scheduled_timestamp
            .cmp(&right.scheduled_timestamp)
            .then_with(|| left.order_id.cmp(&right.order_id))
    });

    let mut buffered_buys = state
        .buffered_buys
        .values()
        .map(|signal| ForwardPaperJournalBufferedBuy {
            source_wallet: signal.source_wallet.clone(),
            source_label: signal.source_label.clone(),
            asset: signal.asset.clone(),
            title: signal.title.clone(),
            first_timestamp: signal.first_timestamp,
            last_timestamp: signal.last_timestamp,
            weighted_price_numerator: signal.weighted_price_numerator,
            buffered_requested_usdc: signal.buffered_requested_usdc,
            buffered_leader_size: signal.buffered_leader_size,
            buffered_leader_usdc: signal.buffered_leader_usdc,
        })
        .collect::<Vec<_>>();
    buffered_buys.sort_by(|left, right| {
        left.source_wallet
            .cmp(&right.source_wallet)
            .then_with(|| left.asset.cmp(&right.asset))
    });

    ForwardPaperJournalState {
        version: FORWARD_PAPER_JOURNAL_VERSION,
        metadata,
        cash: state.cash,
        followed_trades: state.followed_trades,
        ignored_trades: state.ignored_trades,
        closed_trades: state.closed_trades,
        realized_pnl: state.realized_pnl,
        tracked_from_timestamp: state.tracked_from_timestamp,
        tracked_to_timestamp: state.tracked_to_timestamp,
        processed_activity_ids,
        open_positions,
        pending_orders,
        buffered_buys,
        recent_executions: report.recent_executions.clone(),
        skip_reasons: report.skip_reasons.clone(),
    }
}

fn handle_signal(
    source_wallet: &str,
    source_label: Option<String>,
    activity: &WalletActivity,
    config: &SimulationConfig,
    state: &mut EngineState,
) {
    let Some(side) = activity.side else {
        return;
    };

    match side {
        TradeSide::Buy => {
            let requested_usdc =
                (activity.usdc_size * config.wallet_scale).min(config.max_trade_usdc);
            if should_buffer_micro_buy(activity.usdc_size, requested_usdc, config) {
                buffer_micro_buy_signal(
                    source_wallet,
                    source_label,
                    activity,
                    requested_usdc,
                    config,
                    state,
                );
                return;
            }

            queue_pending_buy_order(
                source_wallet,
                source_label,
                activity,
                requested_usdc,
                activity.size,
                config,
                state,
            );
        }
        TradeSide::Sell => {
            let (remaining_sell_size, touched_buffered_buy) =
                reduce_buffered_buys_before_fill(source_wallet, activity, state);
            let (remaining_sell_size, touched_pending_buy) = cancel_pending_buys_before_fill(
                source_wallet,
                &activity.asset,
                activity.timestamp,
                remaining_sell_size,
                config,
                state,
            );

            let position_key = (source_wallet.to_owned(), activity.asset.clone());
            let has_position = state
                .open_positions
                .get(&position_key)
                .map(|position| position.follower_size > EPSILON)
                .unwrap_or(false);

            if !has_position {
                if remaining_sell_size > EPSILON
                    && !touched_buffered_buy
                    && !touched_pending_buy
                    && !has_pending_buy_for_asset(source_wallet, &activity.asset, state)
                {
                    record_execution(
                        state,
                        PortfolioSimulationExecution {
                            source_wallet: source_wallet.to_owned(),
                            source_label,
                            asset: activity.asset.clone(),
                            title: activity.title.clone(),
                            leader_timestamp: activity.timestamp,
                            timestamp: activity.timestamp,
                            side,
                            status: SimulationExecutionStatus::Skipped,
                            requested_usdc: activity.usdc_size * config.wallet_scale,
                            filled_usdc: 0.0,
                            price: bounded_price(activity.price),
                            usdc_size: 0.0,
                            reason: Some("sell_without_followed_position".to_owned()),
                        },
                    );
                }
                return;
            }

            if remaining_sell_size <= EPSILON {
                return;
            }

            let sell_ratio = (remaining_sell_size / activity.size.max(EPSILON)).clamp(0.0, 1.0);
            let requested_usdc =
                (activity.usdc_size * config.wallet_scale * sell_ratio).min(config.max_trade_usdc);
            queue_pending_sell_order(
                source_wallet,
                source_label,
                activity,
                requested_usdc,
                remaining_sell_size,
                config,
                state,
            );
        }
        TradeSide::Unknown => {}
    }
}

fn should_buffer_micro_buy(
    leader_usdc_size: f64,
    requested_usdc: f64,
    config: &SimulationConfig,
) -> bool {
    leader_usdc_size < config.min_leader_trade_usdc || requested_usdc < config.minimum_trade_usdc
}

fn buffer_micro_buy_signal(
    source_wallet: &str,
    source_label: Option<String>,
    activity: &WalletActivity,
    requested_usdc: f64,
    config: &SimulationConfig,
    state: &mut EngineState,
) {
    let key = (source_wallet.to_owned(), activity.asset.clone());
    let price = bounded_price(activity.price);

    {
        let buffer = state
            .buffered_buys
            .entry(key.clone())
            .or_insert_with(|| BufferedBuySignal {
                source_wallet: source_wallet.to_owned(),
                source_label: source_label.clone(),
                asset: activity.asset.clone(),
                title: activity.title.clone(),
                first_timestamp: activity.timestamp,
                last_timestamp: activity.timestamp,
                weighted_price_numerator: 0.0,
                buffered_requested_usdc: 0.0,
                buffered_leader_size: 0.0,
                buffered_leader_usdc: 0.0,
            });

        buffer.last_timestamp = activity.timestamp;
        if buffer.source_label.is_none() {
            buffer.source_label = source_label;
        }
        if buffer.title.is_none() {
            buffer.title = activity.title.clone();
        }
        buffer.weighted_price_numerator += price * requested_usdc;
        buffer.buffered_requested_usdc += requested_usdc;
        buffer.buffered_leader_size += activity.size;
        buffer.buffered_leader_usdc += activity.usdc_size;
    }

    release_buffered_buy_signal(&key, config, state);
}

fn release_buffered_buy_signal(
    key: &(String, String),
    config: &SimulationConfig,
    state: &mut EngineState,
) {
    let mut releasable_orders = Vec::new();

    loop {
        let (release_order, remove_buffer) = {
            let Some(buffer) = state.buffered_buys.get_mut(key) else {
                break;
            };

            if buffer.buffered_requested_usdc + EPSILON < config.minimum_trade_usdc
                || buffer.buffered_leader_usdc + EPSILON < config.min_leader_trade_usdc
            {
                break;
            }

            let release_usdc = buffer.buffered_requested_usdc.min(config.max_trade_usdc);
            let release_ratio =
                (release_usdc / buffer.buffered_requested_usdc.max(EPSILON)).clamp(0.0, 1.0);
            let release_leader_size = buffer.buffered_leader_size * release_ratio;
            let release_leader_usdc = buffer.buffered_leader_usdc * release_ratio;
            let reference_price =
                buffer.weighted_price_numerator / buffer.buffered_requested_usdc.max(EPSILON);

            let release_order = Some((
                buffer.source_wallet.clone(),
                buffer.source_label.clone(),
                buffer.asset.clone(),
                buffer.title.clone(),
                buffer.last_timestamp,
                reference_price,
                release_usdc,
                release_leader_size,
            ));

            buffer.weighted_price_numerator *= 1.0 - release_ratio;
            buffer.buffered_requested_usdc =
                (buffer.buffered_requested_usdc - release_usdc).max(0.0);
            buffer.buffered_leader_size =
                (buffer.buffered_leader_size - release_leader_size).max(0.0);
            buffer.buffered_leader_usdc =
                (buffer.buffered_leader_usdc - release_leader_usdc).max(0.0);
            let remove_buffer =
                buffer.buffered_requested_usdc <= EPSILON || buffer.buffered_leader_size <= EPSILON;
            (release_order, remove_buffer)
        };

        if remove_buffer {
            state.buffered_buys.remove(key);
        }

        if let Some((
            source_wallet,
            source_label,
            asset,
            title,
            leader_timestamp,
            reference_price,
            requested_usdc,
            leader_size,
        )) = release_order
        {
            state.next_order_id += 1;
            releasable_orders.push(PendingOrder {
                order_id: state.next_order_id,
                source_wallet,
                source_label,
                asset,
                title,
                side: TradeSide::Buy,
                leader_timestamp,
                scheduled_timestamp: leader_timestamp + config.follow_delay_secs as i64,
                reference_price,
                requested_usdc,
                initial_requested_usdc: requested_usdc,
                leader_size_remaining: leader_size,
                initial_leader_size: leader_size,
                was_reduced_before_fill: false,
            });
        } else {
            break;
        }
    }

    state.pending_orders.extend(releasable_orders);
}

fn queue_pending_buy_order(
    source_wallet: &str,
    source_label: Option<String>,
    activity: &WalletActivity,
    requested_usdc: f64,
    leader_size: f64,
    config: &SimulationConfig,
    state: &mut EngineState,
) {
    state.next_order_id += 1;
    state.pending_orders.push(PendingOrder {
        order_id: state.next_order_id,
        source_wallet: source_wallet.to_owned(),
        source_label,
        asset: activity.asset.clone(),
        title: activity.title.clone(),
        side: TradeSide::Buy,
        leader_timestamp: activity.timestamp,
        scheduled_timestamp: activity.timestamp + config.follow_delay_secs as i64,
        reference_price: bounded_price(activity.price),
        requested_usdc,
        initial_requested_usdc: requested_usdc,
        leader_size_remaining: leader_size,
        initial_leader_size: leader_size,
        was_reduced_before_fill: false,
    });
}

fn queue_pending_sell_order(
    source_wallet: &str,
    source_label: Option<String>,
    activity: &WalletActivity,
    requested_usdc: f64,
    leader_size: f64,
    config: &SimulationConfig,
    state: &mut EngineState,
) {
    state.next_order_id += 1;
    state.pending_orders.push(PendingOrder {
        order_id: state.next_order_id,
        source_wallet: source_wallet.to_owned(),
        source_label,
        asset: activity.asset.clone(),
        title: activity.title.clone(),
        side: TradeSide::Sell,
        leader_timestamp: activity.timestamp,
        scheduled_timestamp: activity.timestamp + config.follow_delay_secs as i64,
        reference_price: bounded_price(activity.price),
        requested_usdc,
        initial_requested_usdc: requested_usdc,
        leader_size_remaining: leader_size,
        initial_leader_size: leader_size,
        was_reduced_before_fill: false,
    });
}

fn reduce_buffered_buys_before_fill(
    source_wallet: &str,
    activity: &WalletActivity,
    state: &mut EngineState,
) -> (f64, bool) {
    let key = (source_wallet.to_owned(), activity.asset.clone());
    let Some(buffer) = state.buffered_buys.get_mut(&key) else {
        return (activity.size, false);
    };

    let reducible_size = activity.size.min(buffer.buffered_leader_size);
    if reducible_size <= EPSILON {
        return (activity.size, false);
    }

    let reduction_ratio = reducible_size / buffer.buffered_leader_size.max(EPSILON);
    buffer.weighted_price_numerator *= 1.0 - reduction_ratio;
    buffer.buffered_requested_usdc *= 1.0 - reduction_ratio;
    buffer.buffered_leader_usdc *= 1.0 - reduction_ratio;
    buffer.buffered_leader_size = (buffer.buffered_leader_size - reducible_size).max(0.0);

    if buffer.buffered_requested_usdc <= EPSILON || buffer.buffered_leader_size <= EPSILON {
        state.buffered_buys.remove(&key);
    }

    ((activity.size - reducible_size).max(0.0), true)
}

fn cancel_pending_buys_before_fill(
    source_wallet: &str,
    asset: &str,
    timestamp: i64,
    sell_size: f64,
    config: &SimulationConfig,
    state: &mut EngineState,
) -> (f64, bool) {
    let mut remaining_leader_sell = sell_size;
    let mut next_pending_orders = Vec::with_capacity(state.pending_orders.len());
    let mut canceled_executions = Vec::new();
    let mut touched_pending_buy = false;

    for mut order in state.pending_orders.drain(..) {
        let matches_asset = order.side == TradeSide::Buy
            && order.source_wallet == source_wallet
            && order.asset == asset;

        if matches_asset && remaining_leader_sell > EPSILON {
            touched_pending_buy = true;
            let reducible_size = remaining_leader_sell.min(order.leader_size_remaining);
            if reducible_size > EPSILON {
                let original_leader_size = order.leader_size_remaining;
                let remaining_ratio =
                    (original_leader_size - reducible_size) / original_leader_size;
                order.leader_size_remaining = (original_leader_size - reducible_size).max(0.0);
                order.requested_usdc *= remaining_ratio;
                order.was_reduced_before_fill = true;
                remaining_leader_sell -= reducible_size;
            }

            if order.leader_size_remaining <= EPSILON
                || order.requested_usdc < config.minimum_trade_usdc
            {
                canceled_executions.push(PortfolioSimulationExecution {
                    source_wallet: order.source_wallet,
                    source_label: order.source_label,
                    asset: order.asset,
                    title: order.title,
                    leader_timestamp: order.leader_timestamp,
                    timestamp,
                    side: TradeSide::Buy,
                    status: SimulationExecutionStatus::Canceled,
                    requested_usdc: order.initial_requested_usdc,
                    filled_usdc: 0.0,
                    price: order.reference_price,
                    usdc_size: 0.0,
                    reason: Some("leader_unwound_before_fill".to_owned()),
                });
                continue;
            }
        }

        next_pending_orders.push(order);
    }

    state.pending_orders = next_pending_orders;
    for execution in canceled_executions {
        record_execution(state, execution);
    }
    (remaining_leader_sell, touched_pending_buy)
}

fn has_pending_buy_for_asset(source_wallet: &str, asset: &str, state: &EngineState) -> bool {
    state.pending_orders.iter().any(|order| {
        order.side == TradeSide::Buy && order.source_wallet == source_wallet && order.asset == asset
    })
}

fn process_due_orders(
    up_to_timestamp: i64,
    current_marks: &HashMap<String, f64>,
    config: &SimulationConfig,
    state: &mut EngineState,
) {
    if !state
        .pending_orders
        .iter()
        .any(|order| order.scheduled_timestamp <= up_to_timestamp)
    {
        return;
    }

    let mut due_orders = Vec::new();
    let mut remaining_orders = Vec::with_capacity(state.pending_orders.len());

    for order in state.pending_orders.drain(..) {
        if order.scheduled_timestamp <= up_to_timestamp {
            due_orders.push(order);
        } else {
            remaining_orders.push(order);
        }
    }

    due_orders.sort_by(|left, right| {
        left.scheduled_timestamp
            .cmp(&right.scheduled_timestamp)
            .then_with(|| left.order_id.cmp(&right.order_id))
    });

    state.pending_orders = remaining_orders;

    for order in due_orders {
        execute_order(order, current_marks, config, state);
    }
}

fn finalize_buffered_buys(state: &mut EngineState) {
    let buffered = state
        .buffered_buys
        .drain()
        .map(|(_, buffer)| buffer)
        .collect::<Vec<_>>();
    for buffer in buffered {
        record_execution(
            state,
            PortfolioSimulationExecution {
                source_wallet: buffer.source_wallet,
                source_label: buffer.source_label,
                asset: buffer.asset,
                title: buffer.title,
                leader_timestamp: buffer.first_timestamp,
                timestamp: buffer.last_timestamp,
                side: TradeSide::Buy,
                status: SimulationExecutionStatus::Skipped,
                requested_usdc: buffer.buffered_requested_usdc,
                filled_usdc: 0.0,
                price: if buffer.buffered_requested_usdc > EPSILON {
                    buffer.weighted_price_numerator / buffer.buffered_requested_usdc
                } else {
                    0.0
                },
                usdc_size: 0.0,
                reason: Some("buffered_signal_below_minimum".to_owned()),
            },
        );
    }
}

fn execute_order(
    order: PendingOrder,
    current_marks: &HashMap<String, f64>,
    config: &SimulationConfig,
    state: &mut EngineState,
) {
    match order.side {
        TradeSide::Buy => execute_buy(order, current_marks, config, state),
        TradeSide::Sell => execute_sell(order, current_marks, config, state),
        TradeSide::Unknown => {}
    }
}

fn execute_buy(
    order: PendingOrder,
    current_marks: &HashMap<String, f64>,
    config: &SimulationConfig,
    state: &mut EngineState,
) {
    let position_key = (order.source_wallet.clone(), order.asset.clone());
    let existing_position_cost = state
        .open_positions
        .get(&position_key)
        .map(position_cost_basis)
        .unwrap_or_default();
    let total_exposure = total_deployed_cost_basis(&state.open_positions);
    let wallet_exposure = wallet_deployed_cost_basis(&state.open_positions, &order.source_wallet);
    let reserve_target = config.starting_cash * config.cash_reserve_ratio;
    let available_cash = (state.cash - reserve_target).max(0.0);
    let remaining_total_cap =
        (config.starting_cash * config.max_total_exposure_ratio - total_exposure).max(0.0);
    let remaining_wallet_cap =
        (config.starting_cash * config.max_wallet_exposure_ratio - wallet_exposure).max(0.0);
    let remaining_position_cap = (config.starting_cash * config.max_position_exposure_ratio
        - existing_position_cost)
        .max(0.0);
    let is_new_position = existing_position_cost <= EPSILON;

    if is_new_position && state.open_positions.len() >= config.max_open_positions {
        record_execution(
            state,
            PortfolioSimulationExecution {
                source_wallet: order.source_wallet,
                source_label: order.source_label,
                asset: order.asset,
                title: order.title,
                leader_timestamp: order.leader_timestamp,
                timestamp: order.scheduled_timestamp,
                side: TradeSide::Buy,
                status: SimulationExecutionStatus::Skipped,
                requested_usdc: order.initial_requested_usdc,
                filled_usdc: 0.0,
                price: order.reference_price,
                usdc_size: 0.0,
                reason: Some("max_open_positions_reached".to_owned()),
            },
        );
        return;
    }

    let fill_usdc = order
        .requested_usdc
        .min(available_cash)
        .min(remaining_total_cap)
        .min(remaining_wallet_cap)
        .min(remaining_position_cap);

    if fill_usdc < config.minimum_trade_usdc {
        let reason = if available_cash < config.minimum_trade_usdc {
            "cash_reserve_blocked"
        } else if remaining_total_cap < config.minimum_trade_usdc {
            "total_exposure_limit"
        } else if remaining_wallet_cap < config.minimum_trade_usdc {
            "wallet_exposure_limit"
        } else if remaining_position_cap < config.minimum_trade_usdc {
            "position_exposure_limit"
        } else {
            "scaled_trade_below_minimum"
        };
        record_execution(
            state,
            PortfolioSimulationExecution {
                source_wallet: order.source_wallet,
                source_label: order.source_label,
                asset: order.asset,
                title: order.title,
                leader_timestamp: order.leader_timestamp,
                timestamp: order.scheduled_timestamp,
                side: TradeSide::Buy,
                status: SimulationExecutionStatus::Skipped,
                requested_usdc: order.initial_requested_usdc,
                filled_usdc: 0.0,
                price: order.reference_price,
                usdc_size: 0.0,
                reason: Some(reason.to_owned()),
            },
        );
        return;
    }

    let price = bounded_price(apply_buy_slippage(
        conservative_reference_price(
            TradeSide::Buy,
            order.reference_price,
            current_marks.get(&order.asset).copied(),
        ),
        effective_slippage_bps(config, fill_usdc),
    ));
    let fill_ratio = (fill_usdc / order.requested_usdc.max(EPSILON)).clamp(0.0, 1.0);
    let matched_leader_size = order.leader_size_remaining * fill_ratio;
    let follower_size = apply_buy_fee_to_size(fill_usdc / price, config.taker_fee_bps);

    if follower_size <= EPSILON {
        record_execution(
            state,
            PortfolioSimulationExecution {
                source_wallet: order.source_wallet,
                source_label: order.source_label,
                asset: order.asset,
                title: order.title,
                leader_timestamp: order.leader_timestamp,
                timestamp: order.scheduled_timestamp,
                side: TradeSide::Buy,
                status: SimulationExecutionStatus::Skipped,
                requested_usdc: order.initial_requested_usdc,
                filled_usdc: 0.0,
                price,
                usdc_size: 0.0,
                reason: Some("follower_size_too_small_after_fees".to_owned()),
            },
        );
        return;
    }

    let position = state
        .open_positions
        .entry(position_key)
        .or_insert_with(|| PositionState {
            source_wallet: order.source_wallet.clone(),
            source_label: order.source_label.clone(),
            asset: order.asset.clone(),
            title: order.title.clone(),
            leader_size: 0.0,
            follower_size: 0.0,
            avg_entry_price: price,
            last_mark_price: price,
            last_buy_timestamp: order.scheduled_timestamp,
        });

    let total_size = position.follower_size + follower_size;
    let total_cost = (position.avg_entry_price * position.follower_size) + fill_usdc;
    position.avg_entry_price = total_cost / total_size.max(f64::EPSILON);
    position.follower_size = total_size;
    position.leader_size += matched_leader_size;
    position.last_mark_price = current_marks.get(&order.asset).copied().unwrap_or(price);
    position.last_buy_timestamp = order.scheduled_timestamp;

    state.cash -= fill_usdc;

    let reason = if fill_usdc + EPSILON < order.requested_usdc {
        Some(limit_reason(
            order.requested_usdc,
            available_cash,
            remaining_total_cap,
            remaining_wallet_cap,
            remaining_position_cap,
            order.was_reduced_before_fill,
        ))
    } else if order.was_reduced_before_fill {
        Some("leader_reduced_before_fill".to_owned())
    } else {
        None
    };
    let status = if reason.is_some() && fill_usdc + EPSILON < order.initial_requested_usdc {
        SimulationExecutionStatus::Partial
    } else {
        SimulationExecutionStatus::Filled
    };

    record_execution(
        state,
        PortfolioSimulationExecution {
            source_wallet: order.source_wallet,
            source_label: order.source_label,
            asset: order.asset,
            title: order.title,
            leader_timestamp: order.leader_timestamp,
            timestamp: order.scheduled_timestamp,
            side: TradeSide::Buy,
            status,
            requested_usdc: order.initial_requested_usdc,
            filled_usdc: fill_usdc,
            price,
            usdc_size: fill_usdc,
            reason,
        },
    );
}

fn execute_sell(
    order: PendingOrder,
    current_marks: &HashMap<String, f64>,
    config: &SimulationConfig,
    state: &mut EngineState,
) {
    let position_key = (order.source_wallet.clone(), order.asset.clone());
    let Some(position) = state.open_positions.get_mut(&position_key) else {
        record_execution(
            state,
            PortfolioSimulationExecution {
                source_wallet: order.source_wallet,
                source_label: order.source_label,
                asset: order.asset,
                title: order.title,
                leader_timestamp: order.leader_timestamp,
                timestamp: order.scheduled_timestamp,
                side: TradeSide::Sell,
                status: SimulationExecutionStatus::Skipped,
                requested_usdc: order.initial_requested_usdc,
                filled_usdc: 0.0,
                price: order.reference_price,
                usdc_size: 0.0,
                reason: Some("position_not_open_at_fill".to_owned()),
            },
        );
        return;
    };

    if position.leader_size <= EPSILON || position.follower_size <= EPSILON {
        record_execution(
            state,
            PortfolioSimulationExecution {
                source_wallet: order.source_wallet,
                source_label: order.source_label,
                asset: order.asset,
                title: order.title,
                leader_timestamp: order.leader_timestamp,
                timestamp: order.scheduled_timestamp,
                side: TradeSide::Sell,
                status: SimulationExecutionStatus::Skipped,
                requested_usdc: order.initial_requested_usdc,
                filled_usdc: 0.0,
                price: order.reference_price,
                usdc_size: 0.0,
                reason: Some("position_empty_at_fill".to_owned()),
            },
        );
        return;
    }

    let sold_leader_size = order.leader_size_remaining.min(position.leader_size);
    if sold_leader_size <= EPSILON {
        record_execution(
            state,
            PortfolioSimulationExecution {
                source_wallet: order.source_wallet,
                source_label: order.source_label,
                asset: order.asset,
                title: order.title,
                leader_timestamp: order.leader_timestamp,
                timestamp: order.scheduled_timestamp,
                side: TradeSide::Sell,
                status: SimulationExecutionStatus::Skipped,
                requested_usdc: order.initial_requested_usdc,
                filled_usdc: 0.0,
                price: order.reference_price,
                usdc_size: 0.0,
                reason: Some("leader_size_already_unwound".to_owned()),
            },
        );
        return;
    }

    let sell_ratio = (sold_leader_size / position.leader_size).clamp(0.0, 1.0);
    let sold_follower_size = position.follower_size * sell_ratio;
    if sold_follower_size <= EPSILON {
        record_execution(
            state,
            PortfolioSimulationExecution {
                source_wallet: order.source_wallet,
                source_label: order.source_label,
                asset: order.asset,
                title: order.title,
                leader_timestamp: order.leader_timestamp,
                timestamp: order.scheduled_timestamp,
                side: TradeSide::Sell,
                status: SimulationExecutionStatus::Skipped,
                requested_usdc: order.initial_requested_usdc,
                filled_usdc: 0.0,
                price: order.reference_price,
                usdc_size: 0.0,
                reason: Some("follower_size_too_small".to_owned()),
            },
        );
        return;
    }

    let estimated_notional = sold_follower_size * position.avg_entry_price;
    let price = bounded_price(apply_sell_slippage(
        conservative_reference_price(
            TradeSide::Sell,
            order.reference_price,
            current_marks.get(&order.asset).copied(),
        ),
        effective_slippage_bps(config, estimated_notional),
    ));
    let gross_proceeds = sold_follower_size * price;
    let proceeds = apply_sell_fee_to_proceeds(gross_proceeds, config.taker_fee_bps);
    let cost_basis = sold_follower_size * position.avg_entry_price;
    let pnl = proceeds - cost_basis;

    position.follower_size -= sold_follower_size;
    position.leader_size = (position.leader_size - sold_leader_size).max(0.0);
    position.last_mark_price = current_marks.get(&order.asset).copied().unwrap_or(price);

    state.cash += proceeds;
    state.realized_pnl += pnl;
    state.closed_trades += 1;
    state.closed_positions.push(ClosedSimulationTrade {
        source_wallet: Some(order.source_wallet.clone()),
        source_label: order.source_label.clone(),
        asset: order.asset.clone(),
        title: order.title.clone(),
        buy_timestamp: position.last_buy_timestamp,
        sell_timestamp: order.scheduled_timestamp,
        entry_price: position.avg_entry_price,
        exit_price: price,
        size: sold_follower_size,
        pnl,
    });

    if position.follower_size <= EPSILON {
        state.open_positions.remove(&position_key);
    }

    let reason = if sold_leader_size + EPSILON < order.initial_leader_size {
        Some("position_smaller_than_leader_sell".to_owned())
    } else {
        None
    };
    let status = if reason.is_some() {
        SimulationExecutionStatus::Partial
    } else {
        SimulationExecutionStatus::Filled
    };

    record_execution(
        state,
        PortfolioSimulationExecution {
            source_wallet: order.source_wallet,
            source_label: order.source_label,
            asset: order.asset,
            title: order.title,
            leader_timestamp: order.leader_timestamp,
            timestamp: order.scheduled_timestamp,
            side: TradeSide::Sell,
            status,
            requested_usdc: order.initial_requested_usdc,
            filled_usdc: proceeds,
            price,
            usdc_size: proceeds,
            reason,
        },
    );
}

fn record_execution(state: &mut EngineState, execution: PortfolioSimulationExecution) {
    if matches!(
        execution.status,
        SimulationExecutionStatus::Skipped | SimulationExecutionStatus::Canceled
    ) {
        state.ignored_trades += 1;
    } else {
        state.followed_trades += 1;
    }

    if let Some(reason) = &execution.reason {
        *state.skip_reasons.entry(reason.clone()).or_insert(0) += 1;
    }

    state.executions.push(execution);
}

fn build_portfolio_report(
    tracked_wallets: usize,
    current_marks: &HashMap<String, f64>,
    config: &SimulationConfig,
    state: &EngineState,
) -> PortfolioSimulationReport {
    let mut open_positions = state
        .open_positions
        .values()
        .map(|position| {
            let mark_price = current_marks
                .get(&position.asset)
                .copied()
                .unwrap_or(position.last_mark_price);
            let unrealized_pnl = (mark_price - position.avg_entry_price) * position.follower_size;

            PortfolioSimulationPosition {
                source_wallet: position.source_wallet.clone(),
                source_label: position.source_label.clone(),
                asset: position.asset.clone(),
                title: position.title.clone(),
                size: position.follower_size,
                avg_entry_price: position.avg_entry_price,
                mark_price,
                unrealized_pnl,
            }
        })
        .collect::<Vec<_>>();
    open_positions.sort_by(|left, right| right.unrealized_pnl.total_cmp(&left.unrealized_pnl));

    let deployed_cost_basis = open_positions
        .iter()
        .map(|position| position.size * position.avg_entry_price)
        .sum::<f64>();
    let deployed_market_value = open_positions
        .iter()
        .map(|position| position.size * position.mark_price)
        .sum::<f64>();
    let unrealized_pnl = open_positions
        .iter()
        .map(|position| position.unrealized_pnl)
        .sum::<f64>();
    let final_equity = state.cash + deployed_market_value;

    let mut recent_executions = state
        .executions
        .iter()
        .rev()
        .take(RECENT_EXECUTION_LIMIT)
        .cloned()
        .collect::<Vec<_>>();
    recent_executions.sort_by(|left, right| right.timestamp.cmp(&left.timestamp));

    let mut closed_positions = state.closed_positions.clone();
    closed_positions.sort_by(|left, right| right.sell_timestamp.cmp(&left.sell_timestamp));

    let mut skip_reasons = state
        .skip_reasons
        .iter()
        .map(|(reason, count)| SimulationSkipReasonCount {
            reason: reason.clone(),
            count: *count,
        })
        .collect::<Vec<_>>();
    skip_reasons.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.reason.cmp(&right.reason))
    });

    PortfolioSimulationReport {
        tracked_wallets,
        followed_trades: state.followed_trades,
        ignored_trades: state.ignored_trades,
        closed_trades: state.closed_trades,
        realized_pnl: state.realized_pnl,
        unrealized_pnl,
        total_pnl: state.realized_pnl + unrealized_pnl,
        final_cash: state.cash,
        final_equity,
        starting_cash: config.starting_cash,
        tracked_from_timestamp: state.tracked_from_timestamp,
        tracked_to_timestamp: state.tracked_to_timestamp,
        deployed_cost_basis,
        deployed_market_value,
        cash_reserve_target: config.starting_cash * config.cash_reserve_ratio,
        skip_reasons,
        open_positions,
        closed_positions,
        recent_executions,
    }
}

fn total_deployed_cost_basis(positions: &HashMap<(String, String), PositionState>) -> f64 {
    positions.values().map(position_cost_basis).sum()
}

fn wallet_deployed_cost_basis(
    positions: &HashMap<(String, String), PositionState>,
    wallet: &str,
) -> f64 {
    positions
        .values()
        .filter(|position| position.source_wallet == wallet)
        .map(position_cost_basis)
        .sum()
}

fn position_cost_basis(position: &PositionState) -> f64 {
    position.follower_size * position.avg_entry_price
}

fn effective_slippage_bps(config: &SimulationConfig, usdc_size: f64) -> f64 {
    let denominator = config
        .max_trade_usdc
        .max(config.minimum_trade_usdc)
        .max(1.0);
    let impact_ratio = (usdc_size / denominator).clamp(0.0, 1.0);
    config.slippage_bps + (config.impact_slippage_bps * impact_ratio)
}

fn limit_reason(
    requested_usdc: f64,
    available_cash: f64,
    remaining_total_cap: f64,
    remaining_wallet_cap: f64,
    remaining_position_cap: f64,
    leader_reduced_before_fill: bool,
) -> String {
    let mut constraints = Vec::new();
    if available_cash + EPSILON < requested_usdc {
        constraints.push((available_cash, "cash_reserve_blocked"));
    }
    if remaining_total_cap + EPSILON < requested_usdc {
        constraints.push((remaining_total_cap, "total_exposure_limit"));
    }
    if remaining_wallet_cap + EPSILON < requested_usdc {
        constraints.push((remaining_wallet_cap, "wallet_exposure_limit"));
    }
    if remaining_position_cap + EPSILON < requested_usdc {
        constraints.push((remaining_position_cap, "position_exposure_limit"));
    }

    if let Some((_, reason)) = constraints
        .into_iter()
        .min_by(|left, right| left.0.total_cmp(&right.0))
    {
        reason.to_owned()
    } else if leader_reduced_before_fill {
        "leader_reduced_before_fill".to_owned()
    } else {
        "partial_fill".to_owned()
    }
}

fn paper_activity_key(wallet: &str, activity: &WalletActivity) -> String {
    format!(
        "{}:{}:{}:{}:{}:{:.6}:{:.6}",
        wallet,
        activity.transaction_hash.as_deref().unwrap_or("nohash"),
        activity.timestamp,
        activity.asset,
        match activity.side {
            Some(TradeSide::Buy) => "BUY",
            Some(TradeSide::Sell) => "SELL",
            _ => "UNKNOWN",
        },
        activity.price,
        activity.usdc_size,
    )
}

fn bounded_price(price: f64) -> f64 {
    price.clamp(0.001, 0.999)
}

fn conservative_reference_price(
    side: TradeSide,
    reference_price: f64,
    market_price: Option<f64>,
) -> f64 {
    let market_price = market_price.unwrap_or(reference_price);
    match side {
        TradeSide::Buy => reference_price.max(market_price),
        TradeSide::Sell => reference_price.min(market_price),
        TradeSide::Unknown => reference_price,
    }
}

fn apply_buy_slippage(price: f64, slippage_bps: f64) -> f64 {
    price * (1.0 + slippage_bps / 10_000.0)
}

fn apply_sell_slippage(price: f64, slippage_bps: f64) -> f64 {
    price * (1.0 - slippage_bps / 10_000.0)
}

fn apply_buy_fee_to_size(size: f64, taker_fee_bps: f64) -> f64 {
    size * (1.0 - taker_fee_bps.max(0.0) / 10_000.0)
}

fn apply_sell_fee_to_proceeds(proceeds: f64, taker_fee_bps: f64) -> f64 {
    proceeds * (1.0 - taker_fee_bps.max(0.0) / 10_000.0)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::config::SimulationConfig;
    use crate::domain::{SimulationExecutionStatus, TradeSide, WalletActivity, WalletActivityType};

    use super::{
        ForwardPaperJournalMetadata, SharedSimulationInput, advance_forward_paper_journal,
        simulate_copy_trading, simulate_shared_copy_trading,
    };

    #[test]
    fn profitable_round_trip_produces_positive_pnl() {
        let activities = vec![
            WalletActivity {
                proxy_wallet: "0x1111111111111111111111111111111111111111".to_owned(),
                timestamp: 1_700_000_000,
                condition_id: "condition-1".to_owned(),
                activity_type: WalletActivityType::Trade,
                size: 10.0,
                usdc_size: 40.0,
                transaction_hash: None,
                price: 0.4,
                asset: "asset-1".to_owned(),
                side: Some(TradeSide::Buy),
                outcome_index: None,
                title: Some("Will rust win?".to_owned()),
                slug: None,
                event_slug: None,
                outcome: None,
                name: None,
                pseudonym: None,
            },
            WalletActivity {
                proxy_wallet: "0x1111111111111111111111111111111111111111".to_owned(),
                timestamp: 1_700_000_100,
                condition_id: "condition-1".to_owned(),
                activity_type: WalletActivityType::Trade,
                size: 10.0,
                usdc_size: 70.0,
                transaction_hash: None,
                price: 0.7,
                asset: "asset-1".to_owned(),
                side: Some(TradeSide::Sell),
                outcome_index: None,
                title: Some("Will rust win?".to_owned()),
                slug: None,
                event_slug: None,
                outcome: None,
                name: None,
                pseudonym: None,
            },
        ];

        let config = SimulationConfig {
            wallet_scale: 0.5,
            minimum_trade_usdc: 1.0,
            min_leader_trade_usdc: 1.0,
            max_trade_usdc: 50.0,
            slippage_bps: 0.0,
            impact_slippage_bps: 0.0,
            taker_fee_bps: 0.0,
            cash_reserve_ratio: 0.0,
            max_total_exposure_ratio: 1.0,
            max_position_exposure_ratio: 1.0,
            max_wallet_exposure_ratio: 1.0,
            max_open_positions: 10,
            starting_cash: 100.0,
            ..SimulationConfig::default()
        };

        let report = simulate_copy_trading(
            "0x1111111111111111111111111111111111111111",
            &activities,
            &HashMap::new(),
            &config,
        );

        assert!(report.realized_pnl > 0.0);
        assert_eq!(report.closed_trades, 1);
    }

    #[test]
    fn pending_buy_is_canceled_when_leader_sells_before_fill() {
        let wallet = "0x1111111111111111111111111111111111111111";
        let asset = "asset-1";
        let activities = vec![
            WalletActivity {
                proxy_wallet: wallet.to_owned(),
                timestamp: 1_700_000_000,
                condition_id: "condition-1".to_owned(),
                activity_type: WalletActivityType::Trade,
                size: 10.0,
                usdc_size: 40.0,
                transaction_hash: None,
                price: 0.4,
                asset: asset.to_owned(),
                side: Some(TradeSide::Buy),
                outcome_index: None,
                title: Some("Test".to_owned()),
                slug: None,
                event_slug: None,
                outcome: None,
                name: None,
                pseudonym: None,
            },
            WalletActivity {
                proxy_wallet: wallet.to_owned(),
                timestamp: 1_700_000_005,
                condition_id: "condition-1".to_owned(),
                activity_type: WalletActivityType::Trade,
                size: 10.0,
                usdc_size: 35.0,
                transaction_hash: None,
                price: 0.35,
                asset: asset.to_owned(),
                side: Some(TradeSide::Sell),
                outcome_index: None,
                title: Some("Test".to_owned()),
                slug: None,
                event_slug: None,
                outcome: None,
                name: None,
                pseudonym: None,
            },
        ];

        let config = SimulationConfig {
            follow_delay_secs: 10,
            wallet_scale: 0.5,
            minimum_trade_usdc: 1.0,
            min_leader_trade_usdc: 1.0,
            max_trade_usdc: 50.0,
            slippage_bps: 0.0,
            impact_slippage_bps: 0.0,
            taker_fee_bps: 0.0,
            cash_reserve_ratio: 0.0,
            max_total_exposure_ratio: 1.0,
            max_position_exposure_ratio: 1.0,
            max_wallet_exposure_ratio: 1.0,
            max_open_positions: 10,
            starting_cash: 100.0,
        };

        let report = simulate_copy_trading(wallet, &activities, &HashMap::new(), &config);

        assert_eq!(report.followed_trades, 0);
        assert_eq!(report.ignored_trades, 1);
        assert_eq!(report.open_positions.len(), 0);
        assert!(
            report
                .skip_reasons
                .iter()
                .any(|reason| reason.reason == "leader_unwound_before_fill")
        );
    }

    #[test]
    fn shared_simulation_uses_one_cash_pool() {
        let make_activity = |wallet: &str, asset: &str, timestamp: i64| WalletActivity {
            proxy_wallet: wallet.to_owned(),
            timestamp,
            condition_id: format!("condition-{asset}"),
            activity_type: WalletActivityType::Trade,
            size: 10.0,
            usdc_size: 80.0,
            transaction_hash: None,
            price: 0.4,
            asset: asset.to_owned(),
            side: Some(TradeSide::Buy),
            outcome_index: None,
            title: Some(format!("Trade {asset}")),
            slug: None,
            event_slug: None,
            outcome: None,
            name: None,
            pseudonym: None,
        };

        let config = SimulationConfig {
            wallet_scale: 1.0,
            minimum_trade_usdc: 25.0,
            min_leader_trade_usdc: 1.0,
            max_trade_usdc: 100.0,
            slippage_bps: 0.0,
            impact_slippage_bps: 0.0,
            taker_fee_bps: 0.0,
            cash_reserve_ratio: 0.10,
            max_total_exposure_ratio: 1.0,
            max_position_exposure_ratio: 1.0,
            max_wallet_exposure_ratio: 1.0,
            max_open_positions: 10,
            starting_cash: 100.0,
            ..SimulationConfig::default()
        };

        let report = simulate_shared_copy_trading(
            &[
                SharedSimulationInput {
                    source_wallet: "0x1111111111111111111111111111111111111111".to_owned(),
                    source_label: Some("One".to_owned()),
                    activities: vec![make_activity(
                        "0x1111111111111111111111111111111111111111",
                        "asset-1",
                        1_700_000_000,
                    )],
                    current_marks: HashMap::from([("asset-1".to_owned(), 0.4)]),
                },
                SharedSimulationInput {
                    source_wallet: "0x2222222222222222222222222222222222222222".to_owned(),
                    source_label: Some("Two".to_owned()),
                    activities: vec![make_activity(
                        "0x2222222222222222222222222222222222222222",
                        "asset-2",
                        1_700_000_001,
                    )],
                    current_marks: HashMap::from([("asset-2".to_owned(), 0.4)]),
                },
            ],
            &config,
        );

        assert_eq!(report.followed_trades, 1);
        assert_eq!(report.ignored_trades, 1);
        assert_eq!(report.final_cash, 20.0);
    }

    #[test]
    fn position_exposure_limit_creates_partial_fill() {
        let wallet = "0x1111111111111111111111111111111111111111";
        let activities = vec![WalletActivity {
            proxy_wallet: wallet.to_owned(),
            timestamp: 1_700_000_000,
            condition_id: "condition-1".to_owned(),
            activity_type: WalletActivityType::Trade,
            size: 10.0,
            usdc_size: 80.0,
            transaction_hash: None,
            price: 0.4,
            asset: "asset-1".to_owned(),
            side: Some(TradeSide::Buy),
            outcome_index: None,
            title: Some("Cap test".to_owned()),
            slug: None,
            event_slug: None,
            outcome: None,
            name: None,
            pseudonym: None,
        }];

        let config = SimulationConfig {
            wallet_scale: 1.0,
            minimum_trade_usdc: 5.0,
            min_leader_trade_usdc: 1.0,
            max_trade_usdc: 100.0,
            slippage_bps: 0.0,
            impact_slippage_bps: 0.0,
            taker_fee_bps: 0.0,
            cash_reserve_ratio: 0.0,
            max_total_exposure_ratio: 1.0,
            max_position_exposure_ratio: 0.20,
            max_wallet_exposure_ratio: 1.0,
            max_open_positions: 10,
            starting_cash: 100.0,
            ..SimulationConfig::default()
        };

        let report = simulate_shared_copy_trading(
            &[SharedSimulationInput {
                source_wallet: wallet.to_owned(),
                source_label: Some("One".to_owned()),
                activities,
                current_marks: HashMap::from([("asset-1".to_owned(), 0.4)]),
            }],
            &config,
        );

        assert_eq!(report.followed_trades, 1);
        assert_eq!(report.ignored_trades, 0);
        assert_eq!(report.deployed_cost_basis, 20.0);
        assert!(report.recent_executions.iter().any(|execution| {
            execution.status == SimulationExecutionStatus::Partial
                && execution.reason.as_deref() == Some("position_exposure_limit")
        }));
    }

    #[test]
    fn micro_buys_are_buffered_into_one_small_account_fill() {
        let wallet = "0x1111111111111111111111111111111111111111";
        let asset = "asset-1";
        let activities = vec![
            WalletActivity {
                proxy_wallet: wallet.to_owned(),
                timestamp: 1_700_000_000,
                condition_id: "condition-1".to_owned(),
                activity_type: WalletActivityType::Trade,
                size: 10.0,
                usdc_size: 10.0,
                transaction_hash: None,
                price: 0.40,
                asset: asset.to_owned(),
                side: Some(TradeSide::Buy),
                outcome_index: None,
                title: Some("Micro one".to_owned()),
                slug: None,
                event_slug: None,
                outcome: None,
                name: None,
                pseudonym: None,
            },
            WalletActivity {
                proxy_wallet: wallet.to_owned(),
                timestamp: 1_700_000_005,
                condition_id: "condition-1".to_owned(),
                activity_type: WalletActivityType::Trade,
                size: 10.0,
                usdc_size: 10.0,
                transaction_hash: None,
                price: 0.42,
                asset: asset.to_owned(),
                side: Some(TradeSide::Buy),
                outcome_index: None,
                title: Some("Micro two".to_owned()),
                slug: None,
                event_slug: None,
                outcome: None,
                name: None,
                pseudonym: None,
            },
        ];

        let config = SimulationConfig {
            follow_delay_secs: 0,
            wallet_scale: 0.10,
            minimum_trade_usdc: 2.0,
            min_leader_trade_usdc: 0.0,
            max_trade_usdc: 50.0,
            slippage_bps: 0.0,
            impact_slippage_bps: 0.0,
            taker_fee_bps: 0.0,
            cash_reserve_ratio: 0.0,
            max_total_exposure_ratio: 1.0,
            max_position_exposure_ratio: 1.0,
            max_wallet_exposure_ratio: 1.0,
            max_open_positions: 10,
            starting_cash: 100.0,
        };

        let report = simulate_copy_trading(wallet, &activities, &HashMap::new(), &config);

        assert_eq!(report.followed_trades, 1);
        assert_eq!(report.ignored_trades, 0);
        assert_eq!(report.open_positions.len(), 1);
        assert!(report.deployed_cost_basis >= 1.999_999);
        assert!(
            report
                .skip_reasons
                .iter()
                .all(|reason| reason.reason != "scaled_trade_below_minimum")
        );
    }

    #[test]
    fn forward_journal_only_processes_unseen_activity() {
        let wallet = "0x1111111111111111111111111111111111111111";
        let asset = "asset-1";
        let buy = WalletActivity {
            proxy_wallet: wallet.to_owned(),
            timestamp: 1_700_000_000,
            condition_id: "condition-1".to_owned(),
            activity_type: WalletActivityType::Trade,
            size: 10.0,
            usdc_size: 40.0,
            transaction_hash: Some("0xbuy".to_owned()),
            price: 0.4,
            asset: asset.to_owned(),
            side: Some(TradeSide::Buy),
            outcome_index: None,
            title: Some("Journal test".to_owned()),
            slug: None,
            event_slug: None,
            outcome: None,
            name: None,
            pseudonym: None,
        };
        let sell = WalletActivity {
            proxy_wallet: wallet.to_owned(),
            timestamp: 1_700_000_010,
            condition_id: "condition-1".to_owned(),
            activity_type: WalletActivityType::Trade,
            size: 10.0,
            usdc_size: 60.0,
            transaction_hash: Some("0xsell".to_owned()),
            price: 0.6,
            asset: asset.to_owned(),
            side: Some(TradeSide::Sell),
            outcome_index: None,
            title: Some("Journal test".to_owned()),
            slug: None,
            event_slug: None,
            outcome: None,
            name: None,
            pseudonym: None,
        };

        let config = SimulationConfig {
            follow_delay_secs: 0,
            wallet_scale: 0.25,
            minimum_trade_usdc: 2.0,
            min_leader_trade_usdc: 0.0,
            max_trade_usdc: 50.0,
            slippage_bps: 0.0,
            impact_slippage_bps: 0.0,
            taker_fee_bps: 0.0,
            cash_reserve_ratio: 0.0,
            max_total_exposure_ratio: 1.0,
            max_position_exposure_ratio: 1.0,
            max_wallet_exposure_ratio: 1.0,
            max_open_positions: 10,
            starting_cash: 100.0,
        };
        let metadata = ForwardPaperJournalMetadata {
            enabled_wallets: vec![wallet.to_owned()],
            simulation_config: config.clone(),
        };

        let first_progress = advance_forward_paper_journal(
            None,
            &[SharedSimulationInput {
                source_wallet: wallet.to_owned(),
                source_label: Some("One".to_owned()),
                activities: vec![buy.clone()],
                current_marks: HashMap::from([(asset.to_owned(), 0.4)]),
            }],
            metadata.clone(),
            &config,
            1_700_000_000,
        );

        assert_eq!(first_progress.processed_activity_count, 1);
        assert_eq!(first_progress.report.followed_trades, 1);

        let second_progress = advance_forward_paper_journal(
            Some(first_progress.state.clone()),
            &[SharedSimulationInput {
                source_wallet: wallet.to_owned(),
                source_label: Some("One".to_owned()),
                activities: vec![buy.clone()],
                current_marks: HashMap::from([(asset.to_owned(), 0.4)]),
            }],
            metadata.clone(),
            &config,
            1_700_000_005,
        );

        assert_eq!(second_progress.processed_activity_count, 0);
        assert_eq!(second_progress.report.followed_trades, 1);
        assert!(second_progress.new_executions.is_empty());

        let third_progress = advance_forward_paper_journal(
            Some(second_progress.state),
            &[SharedSimulationInput {
                source_wallet: wallet.to_owned(),
                source_label: Some("One".to_owned()),
                activities: vec![buy, sell],
                current_marks: HashMap::from([(asset.to_owned(), 0.6)]),
            }],
            metadata,
            &config,
            1_700_000_010,
        );

        assert_eq!(third_progress.processed_activity_count, 1);
        assert_eq!(third_progress.report.closed_trades, 1);
        assert_eq!(third_progress.report.followed_trades, 2);
    }

    #[test]
    fn buy_fill_uses_worse_current_market_price() {
        let wallet = "0x1111111111111111111111111111111111111111";
        let asset = "asset-1";
        let activities = vec![WalletActivity {
            proxy_wallet: wallet.to_owned(),
            timestamp: 1_700_000_000,
            condition_id: "condition-1".to_owned(),
            activity_type: WalletActivityType::Trade,
            size: 10.0,
            usdc_size: 40.0,
            transaction_hash: None,
            price: 0.4,
            asset: asset.to_owned(),
            side: Some(TradeSide::Buy),
            outcome_index: None,
            title: Some("Conservative fill".to_owned()),
            slug: None,
            event_slug: None,
            outcome: None,
            name: None,
            pseudonym: None,
        }];

        let config = SimulationConfig {
            follow_delay_secs: 0,
            wallet_scale: 1.0,
            minimum_trade_usdc: 2.0,
            min_leader_trade_usdc: 0.0,
            max_trade_usdc: 50.0,
            slippage_bps: 0.0,
            impact_slippage_bps: 0.0,
            taker_fee_bps: 0.0,
            cash_reserve_ratio: 0.0,
            max_total_exposure_ratio: 1.0,
            max_position_exposure_ratio: 1.0,
            max_wallet_exposure_ratio: 1.0,
            max_open_positions: 10,
            starting_cash: 100.0,
        };

        let report = simulate_copy_trading(
            wallet,
            &activities,
            &HashMap::from([(asset.to_owned(), 0.6)]),
            &config,
        );

        assert_eq!(report.open_positions.len(), 1);
        assert!((report.open_positions[0].avg_entry_price - 0.6).abs() < 0.000_001);
    }

    #[test]
    fn buy_fee_reduces_received_position_size() {
        let wallet = "0x1111111111111111111111111111111111111111";
        let asset = "asset-1";
        let activities = vec![WalletActivity {
            proxy_wallet: wallet.to_owned(),
            timestamp: 1_700_000_000,
            condition_id: "condition-1".to_owned(),
            activity_type: WalletActivityType::Trade,
            size: 100.0,
            usdc_size: 50.0,
            transaction_hash: None,
            price: 0.5,
            asset: asset.to_owned(),
            side: Some(TradeSide::Buy),
            outcome_index: None,
            title: Some("Fee test".to_owned()),
            slug: None,
            event_slug: None,
            outcome: None,
            name: None,
            pseudonym: None,
        }];

        let config = SimulationConfig {
            follow_delay_secs: 0,
            wallet_scale: 1.0,
            minimum_trade_usdc: 2.0,
            min_leader_trade_usdc: 0.0,
            max_trade_usdc: 50.0,
            slippage_bps: 0.0,
            impact_slippage_bps: 0.0,
            taker_fee_bps: 100.0,
            cash_reserve_ratio: 0.0,
            max_total_exposure_ratio: 1.0,
            max_position_exposure_ratio: 1.0,
            max_wallet_exposure_ratio: 1.0,
            max_open_positions: 10,
            starting_cash: 100.0,
        };

        let report = simulate_copy_trading(
            wallet,
            &activities,
            &HashMap::from([(asset.to_owned(), 0.5)]),
            &config,
        );

        assert_eq!(report.open_positions.len(), 1);
        assert!((report.open_positions[0].size - 99.0).abs() < 0.000_001);
        assert!(report.open_positions[0].avg_entry_price > 0.5);
    }
}
