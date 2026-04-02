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

## Near-Term Plan

1. ingest wallet activity and market data
2. score candidate wallets
3. simulate copy-trading behavior
4. only consider live execution after paper results are acceptable
