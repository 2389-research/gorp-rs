# Documentation Audit Report

Generated: 2026-02-16 | Branch: `message-bus-refactor` | Commit: `e4c1f92`

## Executive Summary

| Metric | Count |
|--------|-------|
| Documents scanned | 12 |
| Claims verified | ~200 |
| Verified TRUE | ~145 (73%) |
| **Verified FALSE** | **~49 (24%)** |
| Needs human review | ~12 (6%) |

## False Claims Requiring Fixes

### README.md

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 43-51 | Config example uses `[acp]` section with `agent_binary` | Actual section is `[backend]` with fields `type`, `binary` | Update example config block |
| 141 | `acp.agent_binary` defaults to "codex-acp" | Field is `backend.binary`, "codex-acp" not a default | Rewrite config reference table |
| 142 | `acp.timeout_secs` | Actual field is `backend.timeout_secs` | Same as above |
| 180 | `src/config.rs` | Actual: `gorp-core/src/config.rs` | Update file path |
| 181 | `src/session.rs` | Actual: `gorp-core/src/session.rs` | Update file path |
| 183 | `src/acp_client.rs` | Actual: `gorp-agent/src/backends/acp.rs` | Update file path |
| 184 | `src/matrix_client.rs` | Actual: `src/platform/matrix/client.rs` | Update file path |
| 185 | `src/message_handler.rs` (single file) | Actually a module dir with 9 submodules | Describe as module |

### GET-STARTED.md

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 111-131 | Config section `[claude]` with `binary_path` | Actual section is `[backend]` with field `binary` | Update example config |
| 175 | `!join projectname` creates a new room | `!join` invites to existing channel; `!create` creates | Fix command description |
| 191 | `!clear` command exists | No `!clear` command; use `!reset` instead | Remove or replace with `!reset` |

### docs/DOCKER.md

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 20 | `CLAUDE_BINARY_PATH` env var | Not used in code; correct var is `BACKEND_BINARY` | Update env var name |
| 41 | Uncomment volume line in `docker-compose.yml` | No commented-out claude volume mount exists | Rewrite Docker volume instructions |
| 59-65 | Install Node.js `setup_20.x`, `@anthropic-ai/claude-cli` | Dockerfile uses `setup_22.x`, package is `@anthropic-ai/claude-code` | Update package names/versions |
| 69-70 | Volumes `./sessions_db` and `./crypto_store` | Actual volumes use `./app-data/*` structure | Update volume paths |
| 86, 98 | Docker service name `matrix-bridge` | Service is named `gorp` | Replace `matrix-bridge` with `gorp` |
| 103 | Container user is `claude` | Dockerfile creates user `gorp` | Replace `claude` with `gorp` |

### docs/HELP.md

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 3 | "gorp is a Matrix-Claude bridge" | Multi-platform bridge (Matrix, Telegram, Slack) | Update description |
| 138 | GitHub URL `https://github.com/2389-research/gorp` | Missing `-rs` suffix; actual repo is `gorp-rs` | Fix URL |
| 139 | Issues URL `https://github.com/2389-research/gorp/issues` | Same `-rs` suffix missing | Fix URL |

### docs/MOTD.md

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 1 | `gorp v0.2.1` | `Cargo.toml` shows `0.3.2` | Update version |

### docs/CHANGELOG.md

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 5 | Latest version `[0.2.1]` | `Cargo.toml` shows `0.3.2` | Add entries for 0.2.2-0.3.2 |
| 23 | Renamed `.matrix` to `.gorp` throughout | `src/mcp.rs` still uses `.matrix` in 3 places | Complete the rename |

### docs/testing.md

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 14 | Session persistence with "sled database" | Uses SQLite (rusqlite), NOT sled | Replace "sled" with "SQLite" throughout |
| 15 | `tests/claude_tests.rs` exists | File does not exist | Remove reference or create file |
| 25 | Test name `config_loads_from_env` | Actual: `test_config_loads_from_toml_file` | Update test name |
| 34-50 | 5 scenario test scripts in `.scratch/` | None of these `.sh` files exist | Remove section or create scripts |
| 50 | `.scratch/run_all_scenarios.sh` | File does not exist | Same as above |
| 145, 155 | "sled database" / "Sled (on-disk key-value store)" | SQLite, not sled | Replace all sled references |
| 223 | Test name `session_create_and_load` | Actual: `test_channel_create_and_load` | Update test name |

### packaging/README.md

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 59 | Homebrew formula at `homebrew/gorp.rb` | Actual: `packaging/homebrew/gorp.rb` | Update path |

### CLAUDE.md

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 25-30 | Architecture lists only `gorp-agent/` and `src/` | Missing `gorp-core/` and `gorp-ffi/` workspace members | Add missing crates |

### docs/examples/matrix-bridge/README.md

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 3-4 | "spawning the `claude` CLI for every inbound message" | Only spawns for `!claude`-prefixed messages | Clarify trigger condition |
| 10 | "Listens to a single room" | Room filtering is optional, off by default | Say "optionally filter to a single room" |
| 11 | "or when the bot is mentioned" | No mention detection in code | Remove mention claim |
| 25, 52 | `cd examples/matrix-bridge` | Actual path: `docs/examples/matrix-bridge` | Fix path |

### docs/examples/simple-tui/README.md

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 18, 41 | `cd examples/simple-tui` | Actual path: `docs/examples/simple-tui` | Fix path |
| 55 | CLI spawned with `--input-format text` | Neither script passes this flag | Remove from docs |
| 59-60 | "pipes user messages through stdin" | Messages passed as CLI positional args | Fix description |

### claude-jail/README.md

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| -- | (entire file is empty) | 0 bytes, no documentation | Write README or remove directory |

## Pattern Summary

| Pattern | Count | Root Cause |
|---------|-------|------------|
| Wrong config section (`[acp]`/`[claude]` vs `[backend]`) | 6 | Section renamed, docs not updated |
| Wrong file paths (`src/` vs `gorp-core/src/`, `gorp-agent/src/`) | 6 | Monorepo restructured into workspace crates |
| Wrong example paths (`examples/` vs `docs/examples/`) | 4 | Examples moved to `docs/`, README paths stale |
| Sled references (should be SQLite) | 5 | Storage backend changed, testing.md never updated |
| Docker naming (`matrix-bridge`/`claude` vs `gorp`) | 3 | Service/user renamed from matrix-bridge/claude to gorp |
| Missing/phantom test files | 7 | Scenario scripts documented but never created |
| Stale CHANGELOG | 1 | 10+ versions behind current |
| Incomplete rename (`.matrix` to `.gorp`) | 1 | Rename started but `src/mcp.rs` still uses `.matrix` |

## Human Review Queue

- [ ] README.md line 3: "via Matrix" description -- now multi-platform, may want broader description
- [ ] README.md line 17: "Rust 1.70+" -- no `rust-version` in Cargo.toml to verify
- [ ] README.md line 18-25: ACP prerequisites -- only relevant for "acp" backend, "mux" is now default
- [ ] README.md line 161: `crypto_store/` path -- actual path is `~/.local/share/gorp/crypto_store/`
- [ ] GET-STARTED.md line 7: "chat with Claude through Matrix" -- now multi-platform
- [ ] CHANGELOG.md line 17-19: Workspace template contents -- need manual inspection
- [ ] CHANGELOG.md line 28: `[claude]` config section fix -- section may have been renamed
- [ ] docs/testing.md line 205-206: CLI arg format `--session-id <uuid>` -- verify exact flag name
- [ ] docs/HELP.md line 119: Workspace structure files -- verify all listed files exist in template

## Comparison with Prior Audit (2026-02-11)

Several issues flagged in the 2026-02-11 audit remain unfixed:
- `CLAUDE_BINARY_PATH` env var (should be `BACKEND_BINARY`)
- `config.toml.example` line 10 says `channels.db` (code uses `sessions.db`)
- Docker service name `matrix-bridge` (should be `gorp`)
- Container user `claude` (should be `gorp`)
