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

The main app now includes a shared paper simulation that uses one bankroll across all
watchlist wallets with `paper_follow_enabled = true`.

## Near-Term Plan

1. ingest wallet activity and market data
2. score candidate wallets
3. simulate copy-trading behavior
4. only consider live execution after paper results are acceptable
