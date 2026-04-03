# mochi_gain_hunter

Rust bot for researching and automating Polymarket wallet-following strategies.

## Goal

`mochi_gain_hunter` is intended to:

- identify historically strong-performing Polymarket wallets
- track their market activity and timing
- score whether their trades are worth following
- simulate follower trades before any live execution

## Status

The repository is public and now contains the project source directly.

## App

Launch the full-screen terminal app:

```bash
cargo run
```

The default app now includes:

- `Leaderboard` tab with public Polymarket discovery
- `Watchlist` tab with saved wallets and recent trades
- `Wallet` tab for detailed inspection
- `Paper` tab with a shared `$100` paper account across enabled wallets

Useful controls:

- `Tab` / `Shift+Tab` or `h` / `l`: switch tabs
- `j` / `k` or arrows: move selection
- `Enter`: open wallet actions
- `i`: inspect selected wallet
- `a`: add selected leaderboard wallet to watchlist
- `p`: toggle paper-follow for the selected wallet
- `d`: remove the selected watchlist wallet
- `c` / `t` / `o`: cycle leaderboard category, time period, and ordering
- `r`: refresh current data
- `q`: quit

Wallet actions from the app let you:

- inspect a leaderboard or watchlist wallet
- add/remove it from the watchlist
- start or stop paper-following without editing config by hand

## Paper Model

The paper account is no longer a simple optimistic replay. It now tries to behave more
like a live follower:

- uses one shared bankroll across all enabled wallets
- applies delayed pending orders instead of immediate fills
- uses conservative market-side pricing, so paper fills do not beat the current market mark
- aggregates micro leader buys into executable follower-sized orders for small accounts
- cancels or shrinks buys if the leader unwinds before our delayed fill
- enforces a cash reserve, total exposure cap, per-wallet cap, per-position cap, and max open positions
- applies base slippage plus additional size-based impact slippage
- applies configurable taker-fee friction to copied trades
- records skipped and partial-fill reasons so you can see why trades were not copied cleanly

This is still an approximation. Without historical order book depth, exact queue position,
and real-time market snapshots, it cannot perfectly reproduce live fills. It is materially
closer than the previous model, but it is not a guarantee of live execution quality.

## Debug Commands

Initialize a local config file:

```bash
cargo run -- init-config
```

Discover candidate wallets from the public Polymarket leaderboard:

```bash
cargo run -- discover --category OVERALL --time-period MONTH --order-by PNL --limit 10
```

Inspect one wallet in detail:

```bash
cargo run -- inspect-wallet 0x0123456789abcdef0123456789abcdef01234567
```

Run a paper copy-trade simulation for one wallet:

```bash
cargo run -- simulate-follow 0x0123456789abcdef0123456789abcdef01234567
```

Start the older live wallet monitor dashboard:

```bash
cargo run -- monitor
```

Monitor a specific wallet, `@handle`, or Polymarket profile URL:

```bash
cargo run -- monitor "https://polymarket.com/@0xde17f7144fbd0eddb2679132c10ff5e74b120988-1772205225932"
cargo run -- monitor "@gamblingisallyouneed"
```

Emit one non-interactive JSON monitoring snapshot:

```bash
cargo run -- monitor --plain --cycles 1
```

Run the headless background service once:

```bash
cargo run -- service --once
```

Run the headless background service continuously:

```bash
cargo run -- service
```

Backtest a wallet across a small parameter grid:

```bash
cargo run -- backtest-wallet "@gamblingisallyouneed" --top 5
```

The current implementation uses public Polymarket endpoints for leaderboard, activity,
positions, closed positions, and midpoint prices. It does not place live orders yet.
Live monitoring for other wallets currently uses public Data API polling and a Ratatui
dashboard.

## Tracking Data

The monitor now persists tracking data under `data/`:

- `data/history/`: refresh snapshots
- `data/latest/`: latest report per wallet
- `data/activities/`: appended trade activity logs used by backtesting
- `data/paper_account/latest.json`: latest shared paper-account snapshot
- `data/paper_account/history/shared_account.jsonl`: paper-account history for long-running use
- `data/paper_account/forward_state.json`: resumable forward-only paper state
- `data/paper_account/history/journal.jsonl`: append-only paper execution journal
- `data/service/status.json`: latest headless service heartbeat
- `data/service/history/status.jsonl`: service heartbeat history
- `data/service/alerts/latest.json`: latest emitted service alerts
- `data/service/alerts/history/alerts.jsonl`: append-only service alert history

The main app now includes a shared paper simulation that uses one bankroll across all
watchlist wallets with `paper_follow_enabled = true`.

The default config also includes an `[http]` section for request timeout and retry behavior,
plus `simulation.taker_fee_bps` for extra execution realism. It also includes `[service]`
and `[alerts]` sections for headless polling, replay suppression, stdout alerts, and optional
desktop notifications through `notify-send`.

## 24/7 Use

The app is now better suited for long-running use because the shared paper account no longer
needs a full replay on each refresh. It persists a resumable forward-only journal and advances
from locally stored wallet activity, with a small overlap window to avoid missing edge-case
events around restarts. API calls now use retry plus backoff, and both the TUI and the
headless service keep running on refresh errors instead of exiting on a single timeout.

For 24/7 running, use the new `service` command instead of the TUI. It:

- refreshes the watchlist on a fixed interval
- reuses stale wallet rows when one wallet times out
- advances the forward-only paper journal
- emits execution-style alerts for `FILLED`, `PARTIAL`, `CANCELED`, and configured risk skips
- writes heartbeat and alert history to `data/service/`

If you want desktop notifications, set:

```toml
[alerts]
desktop_notifications = true
desktop_command = "notify-send"
```

For a persistent user service, see [docs/SERVICE.md](/home/xler/Projects/mochi_gain_hunter/docs/SERVICE.md) and the unit example [docs/mochi_gain_hunter.service](/home/xler/Projects/mochi_gain_hunter/docs/mochi_gain_hunter.service).

## Near-Term Plan

1. ingest wallet activity and market data
2. score candidate wallets
3. simulate copy-trading behavior
4. only consider live execution after paper results are acceptable
