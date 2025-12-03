# Testing Guide

## Overview

This project uses a multi-layered testing approach combining unit tests, integration tests, and scenario tests to ensure reliability and correctness.

## Test Categories

### 1. Unit Tests (Cargo Test)

Located in `tests/` directory, these test individual modules in isolation:

- `tests/config_tests.rs` - Configuration parsing and validation
- `tests/session_tests.rs` - Session persistence with sled database
- `tests/claude_tests.rs` - Claude CLI response parsing

**Run unit tests:**
```bash
cargo test
```

**Run specific test:**
```bash
cargo test config_loads_from_env
```

**Run with output:**
```bash
cargo test -- --nocapture
```

### 2. Scenario Tests (Real Dependencies)

Located in `.scratch/` directory (gitignored), these test real system integration:

- `test_session_persistence.sh` - Real sled database operations
- `test_config_parsing.sh` - Real environment variable parsing
- `test_claude_json_parsing.sh` - Real JSON parsing with sample responses
- `test_file_operations.sh` - Real filesystem I/O and directory creation
- `test_cli_args_generation.sh` - CLI argument generation validation

**Key principles:**
- **NO MOCKS** - All tests use real dependencies (real sled DB, real files, real env vars)
- **Observable outcomes** - Tests verify actual system behavior
- **Independent** - Scenarios can run in any order
- **Self-cleaning** - Tests clean up after themselves

**Run all scenarios:**
```bash
.scratch/run_all_scenarios.sh
```

**Run individual scenario:**
```bash
.scratch/test_session_persistence.sh
```

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

### Session Persistence (`test_session_persistence.sh`)

**What it tests:**
- Session creation with real sled database
- Session retrieval from disk
- Session state updates (marking as started)
- Persistence across process restarts
- Real database files on disk

**Real dependencies used:**
- Sled database (on-disk key-value store)
- Filesystem operations
- Process spawning

### Config Parsing (`test_config_parsing.sh`)

**What it tests:**
- Environment variable parsing
- Required field validation
- Default value application
- CSV parsing with whitespace
- Empty string filtering

**Real dependencies used:**
- Real environment variables
- Shell process environment

### Claude JSON Parsing (`test_claude_json_parsing.sh`)

**What it tests:**
- Multi-block text responses
- Single block responses
- Empty content arrays
- Mixed content types (filtering non-text)
- Invalid JSON rejection
- Multiline responses
- Unicode character handling

**Real dependencies used:**
- Real JSON samples from Claude API
- serde_json parser

### File Operations (`test_file_operations.sh`)

**What it tests:**
- Sled database directory creation
- File writing and persistence
- Read/write consistency
- Multiple rooms in same database
- Directory permissions
- Concurrent database access

**Real dependencies used:**
- Real filesystem
- Real sled database
- Real concurrent processes

### CLI Args Generation (`test_cli_args_generation.sh`)

**What it tests:**
- First message args (`--session-id <uuid>`)
- Continuation args (`--resume <uuid>`)
- Command line construction
- Different rooms have different session IDs

**Real dependencies used:**
- Real session store
- Real command line construction

## Debugging Tests

### Enable verbose logging:
```bash
RUST_LOG=debug cargo test -- --nocapture
```

### Run single test with backtrace:
```bash
RUST_BACKTRACE=1 cargo test session_create_and_load
```

### Inspect scenario test intermediate files:
Scenario tests create temporary directories. To inspect:
1. Edit scenario script
2. Comment out `rm -rf "$TEST_DIR"` in cleanup function
3. Run test
4. Inspect files in printed temp directory

## Continuous Integration

All tests should pass before merging:

```bash
# Unit tests
cargo test

# Scenario tests
.scratch/run_all_scenarios.sh

# Linting
cargo clippy

# Formatting
cargo fmt --check
```

## Test Coverage

Current coverage areas:
- ✅ Configuration parsing and validation
- ✅ Session persistence with sled
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
6. Add to `run_all_scenarios.sh`

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

**Tests fail with sled errors:**
- Ensure `/tmp` has write permissions
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

**Mocks hide bugs.** When you mock a database, you test your mock, not your code. Scenario tests use real sled databases, real files, and real processes to catch integration issues that mocks would miss.

### Scenario vs Unit

- **Unit tests**: Fast, focused, test logic in isolation
- **Scenario tests**: Realistic, catch integration bugs, verify real system behavior
- Both are necessary for confidence

### Observable Outcomes

Tests verify what the system actually does (files created, data persisted, processes spawned) rather than internal state. This catches real-world bugs.

## Resources

- [Cargo Test Documentation](https://doc.rust-lang.org/cargo/commands/cargo-test.html)
- [Sled Database](https://github.com/spacejam/sled)
- [Matrix SDK Testing](https://matrix-org.github.io/matrix-rust-sdk/)
- [Scenario Testing Philosophy](https://www.hillelwayne.com/post/cross-branch-testing/)
