# Testing Guide

## Overview

This project uses a multi-layered testing approach combining unit tests, integration tests, and scenario tests to ensure reliability and correctness.

## Test Categories

### 1. Unit Tests (Cargo Test)

Located in `tests/` directory, these test individual modules in isolation:

- `tests/config_tests.rs` - Configuration parsing and validation
- `tests/session_tests.rs` - Session persistence with SQLite database

**Run unit tests:**
```bash
cargo test
```

**Run specific test:**
```bash
cargo test test_config_loads_from_toml_file
```

**Run with output:**
```bash
cargo test -- --nocapture
```

### 2. Scenario Tests (Real Dependencies)

The `.scratch/` directory (gitignored) can be used for ad-hoc scenario tests that exercise real system integration. Any shell scripts placed there will not be committed to version control.

**Key principles:**
- **NO MOCKS** - All tests use real dependencies (real SQLite DB, real files, real env vars)
- **Observable outcomes** - Tests verify actual system behavior
- **Independent** - Scenarios can run in any order
- **Self-cleaning** - Tests clean up after themselves

### 3. Integration Tests (Manual)

End-to-end testing with real Matrix and Claude instances:

1. Copy `.env.example` to `.env`
2. Configure with real Matrix credentials
3. Create a test room with E2E encryption enabled
4. Add your bot user to the room
5. Add your personal user ID to `ALLOWED_USERS`

**First Run:**

```bash
cargo run --release
```

Expected output:
```
INFO Starting Matrix-Claude Bridge
INFO Configuration loaded
INFO Session store initialized
INFO Logged in successfully
INFO Joined room
INFO Message handler registered, starting sync loop
```

## Test Scenarios

### Device Verification

On first run, the bot creates a new device. You must verify it:

1. Open Element (or another Matrix client)
2. Go to Settings → Security → Verify this device
3. Complete emoji verification or cross-signing
4. Return to terminal and send a message

### Test Case 1: Authorized User Message

**Action:** Send "Hello Claude!" from whitelisted user

**Expected:**
- Bot sets typing indicator
- Claude CLI is invoked
- Response appears in room
- Logs show session ID created

### Test Case 2: Follow-up Message

**Action:** Send "What did I just say?"

**Expected:**
- Bot uses `--resume` with same session ID
- Claude has context from previous message
- Response references "Hello Claude!"

### Test Case 3: Unauthorized User

**Action:** Add another user to room (not in whitelist), send message as that user

**Expected:**
- Bot ignores message silently
- Logs show "Ignoring message from unauthorized user"

### Test Case 4: Bot Restart Persistence

**Action:** Stop bot (Ctrl+C), restart, send message

**Expected:**
- Bot resumes with same session ID
- Context is preserved

### Test Case 5: Decryption Test

**Action 1:** Send encrypted message from verified device

**Expected:**
- Bot decrypts and processes message normally

**Action 2:** Send from unverified device

**Expected:**
- Bot logs decryption failure
- Sends error message to room

## Scenario Test Details

Scenario tests can be added to the `.scratch/` directory as shell scripts. They should
exercise real dependencies (SQLite database, filesystem, environment variables) and
verify observable outcomes. See the "Adding New Tests" section below for the recommended
structure.

## Debugging Tests

### Enable verbose logging:
```bash
RUST_LOG=debug cargo test -- --nocapture
```

### Run single test with backtrace:
```bash
RUST_BACKTRACE=1 cargo test test_channel_create_and_load
```

### Inspect scenario test intermediate files:
If scenario tests create temporary directories, inspect them by:
1. Editing the scenario script
2. Commenting out cleanup in the trap function
3. Running the test
4. Inspecting files in the printed temp directory

## Continuous Integration

All tests should pass before merging:

```bash
# Unit tests
cargo test

# Linting
cargo clippy

# Formatting
cargo fmt --check
```

## Test Coverage

Current coverage areas:
- ✅ Configuration parsing and validation
- ✅ Session persistence with SQLite
- ✅ Claude JSON response parsing
- ✅ File I/O and directory operations
- ✅ CLI argument generation
- ✅ Multi-room session isolation
- ⚠️  Matrix client initialization (manual only)
- ⚠️  E2E encryption (manual only)
- ⚠️  Message handling flow (manual only)

## Adding New Tests

### Unit Test
1. Create test in `tests/` directory
2. Follow TDD: write test first, see it fail, implement, see it pass
3. Test one specific behavior
4. Use standard Rust test framework

### Scenario Test
1. Create bash script in `.scratch/`
2. Use REAL dependencies (no mocks)
3. Verify observable outcomes
4. Clean up resources in trap
5. Make script executable

**Example scenario test structure:**
```bash
#!/bin/bash
set -euo pipefail

TEST_DIR=$(mktemp -d)
cleanup() { rm -rf "$TEST_DIR"; }
trap cleanup EXIT

# Test with real dependencies
# Verify observable outcomes
# Exit 0 on success, 1 on failure
```

## Troubleshooting

**Tests fail with SQLite errors:**
- Ensure the workspace directory is writable
- Check disk space
- Clean old test databases: `cargo clean`

**Scenario tests fail to compile:**
- Tests fallback to `cargo test` automatically
- Check that `cargo build --release` succeeds first

**Unit tests pass but integration fails:**
- Check `.env` configuration
- Verify Matrix credentials
- Ensure Claude CLI is authenticated: `claude auth status`

## Philosophy

### Why No Mocks?

**Mocks hide bugs.** When you mock a database, you test your mock, not your code. Scenario tests use real SQLite databases, real files, and real processes to catch integration issues that mocks would miss.

### Scenario vs Unit

- **Unit tests**: Fast, focused, test logic in isolation
- **Scenario tests**: Realistic, catch integration bugs, verify real system behavior
- Both are necessary for confidence

### Observable Outcomes

Tests verify what the system actually does (files created, data persisted, processes spawned) rather than internal state. This catches real-world bugs.

## Resources

- [Cargo Test Documentation](https://doc.rust-lang.org/cargo/commands/cargo-test.html)
- [rusqlite (SQLite for Rust)](https://github.com/rusqlite/rusqlite)
- [Matrix SDK Testing](https://matrix-org.github.io/matrix-rust-sdk/)
- [Scenario Testing Philosophy](https://www.hillelwayne.com/post/cross-branch-testing/)
