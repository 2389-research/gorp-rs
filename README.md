# Matrix-Claude Bridge

Rust bot that bridges Matrix room messages to Claude Code CLI.

## Setup

1. Copy `.env.example` to `.env` and configure
2. Build: `cargo build --release`
3. Run: `cargo run --release`

## Configuration

See `.env.example` for all options.

## First Run

The bot creates a new Matrix device on first login. You must verify this device from another Matrix client (Element, etc.) using emoji verification or cross-signing.
