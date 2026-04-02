# mochi_gain_hunter

Public-facing repository for the `mochi_gain_hunter` project.

## What This Repo Is

This repository is intentionally public for visibility, planning, and documentation.
The actual Rust source code for the bot lives in a separate private repository:
`mochi_gain_hunter_private`.

That split is required because a single public repository cannot keep tracked source
files private.

## Project Goal

`mochi_gain_hunter` is planned as a Rust bot that:

- identifies historically strong-performing Polymarket wallets
- tracks their activity and timing
- evaluates whether their trades are worth following
- optionally mirrors selected trades under strict risk controls

## Current Status

The private source repository has been initialized locally and is ready for the
first implementation pass.

## Suggested Hosting Layout

- Public GitHub repo: `mochi_gain_hunter`
- Private GitHub repo: `mochi_gain_hunter_private`

Use the public repo for:

- README and architecture notes
- roadmap and issue tracking
- screenshots, metrics snapshots, and release notes

Use the private repo for:

- Rust source code
- API integrations and trading logic
- keys, configs, and deployment assets

## Next Step

Implement the first private-source MVP:

1. ingest wallet activity and market data
2. score candidate wallets
3. simulate copy-trading before any live execution
