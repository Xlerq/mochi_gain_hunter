# Service Mode

`mochi_gain_hunter` now has a headless `service` mode for 24/7 paper-following:

```bash
cargo run -- service
```

Useful variants:

```bash
cargo run -- service --once
cargo run -- service --cycles 5
```

What it does each cycle:

- refreshes watched wallets from public Polymarket APIs
- falls back to stale in-memory wallet rows when one wallet times out
- persists wallet activity and snapshots
- advances the forward-only shared paper journal
- emits execution-style alerts for new paper decisions
- routes actionable decisions through the executor boundary
- writes service heartbeat and alert history under `data/service/`

## Service Files

- `data/service/status.json`: latest heartbeat
- `data/service/history/status.jsonl`: heartbeat history
- `data/service/alerts/latest.json`: latest emitted alerts
- `data/service/alerts/history/alerts.jsonl`: alert history
- `data/execution/latest.json`: latest executor receipts
- `data/execution/history/receipts.jsonl`: executor receipt history

## Executor Boundary

The current executor mode is `PAPER`. It does not place live orders.

It receives only actionable intents derived from paper decisions:

- `FILLED`
- `PARTIAL` when `execution.submit_partial = true`

That executor then records receipts which can later be replaced by a real Polymarket
execution adapter without changing the service loop itself.

## Alerts

Default behavior:

- print alerts to stdout
- persist alerts to disk
- suppress replay alerts when the paper journal has to rebuild from history
- do not send desktop notifications unless enabled in config

Desktop notifications can be enabled with:

```toml
[alerts]
desktop_notifications = true
desktop_command = "notify-send"
```

## systemd User Service

Build the release binary first:

```bash
cargo build --release
```

Copy the unit example from [mochi_gain_hunter.service](/home/xler/Projects/mochi_gain_hunter/docs/mochi_gain_hunter.service)
to `~/.config/systemd/user/mochi_gain_hunter.service`, then run:

```bash
systemctl --user daemon-reload
systemctl --user enable --now mochi_gain_hunter.service
systemctl --user status mochi_gain_hunter.service
journalctl --user -u mochi_gain_hunter.service -f
```
