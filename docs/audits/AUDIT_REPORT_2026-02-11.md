# Documentation Audit Report

Generated: 2026-02-11 | Commit: 209b77e

## Executive Summary

| Metric | Count |
|--------|-------|
| Documents scanned | 12 |
| Claims verified | ~125 |
| Verified TRUE | ~89 (71%) |
| **Verified FALSE** | **~36 (29%)** |

---

## False Claims Requiring Fixes

### README.md

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 17 | Rust 1.70+ required | Cargo.toml uses edition 2021 (needs Rust 1.56+), but deps may need higher. Dockerfile says Rust 1.85+. | Verify minimum and update |
| 18-25 | ACP adapter required (`codex-acp` or `claude-code-acp`) | Default backend is now `mux` (native Rust). ACP is one option among several. | Update prerequisites to reflect mux as default |
| 50-51 | Config section `[acp]` with `agent_binary` and `timeout_secs` | Actual section is `[backend]` with `type`, `binary`, `timeout_secs` | Update config example to use `[backend]` |
| 94 | Template contains `.mcp-servers.json` | Actual file is `.mcp.json` | Change to `.mcp.json` |
| 141 | `acp.agent_binary` defaults to "codex-acp" | Backend defaults to type "acp", binary defaults to "claude-code-acp" per config.toml.example | Update default name |
| 173 | `workspace/sessions.db` | Actual filename is `sessions.db` in workspace. But config.toml.example line 10 says `channels.db`. Code uses `sessions.db` | Fix config.toml.example reference |
| 180-186 | Code structure lists `src/config.rs`, `src/session.rs`, `src/acp_client.rs`, `src/matrix_client.rs` | These files don't exist. Actual structure is modular: `src/platform/matrix/client.rs`, `gorp-core/src/session.rs`, `gorp-agent/src/backends/acp.rs`, config is in `gorp-agent/src/config.rs` | Rewrite code structure section |

### GET-STARTED.md

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 123-124 | Config section `[claude]` with `binary_path = "claude"` | Actual section is `[backend]` with `type` and `binary` fields | Update to `[backend]` section |
| 129 | Config section `[scheduler]` with `timezone` | Exists and correct | No fix needed |
| 139-144 | Env vars include `MATRIX_RECOVERY_KEY`, `ALLOWED_USERS` | These exist but `CLAUDE_BINARY_PATH` is NOT a valid env var (should be `BACKEND_BINARY`) | N/A for these, but check other docs |
| 153 | `gorp start` command | Valid - exists in main.rs | No fix needed |
| 175 | `!join projectname` creates a room | Command exists but README.md uses `!create` for this. Both exist. | Clarify difference between `!create` and `!join` |
| 189 | `!schedule every day at 9am: Good morning!` | Schedule command exists. Colon syntax unverified. | Verify colon syntax |
| 191 | `!clear` command | **Does not exist** in the codebase | Remove or replace with `!reset` |

### docs/MOTD.md

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 1 | `gorp v0.2.1` | Cargo.toml says `version = "0.3.2"` | Update to v0.3.2 |

### docs/DOCKER.md

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 20 | Env var `CLAUDE_BINARY_PATH=/usr/local/bin/claude` | No such env var. Backend binary is `BACKEND_BINARY` | Update env var name |
| 57-64 | Install Claude via `npm install -g @anthropic-ai/claude-cli` | Package name may be `@anthropic-ai/claude-code` now | Verify and update package name |
| 69-71 | Persistent volumes: `./sessions_db`, `./crypto_store` | Actual paths in docker-compose.yml: `./app-data/data`, `./app-data/config`, etc. | Update to actual volume paths |
| 86 | Service name `matrix-bridge` | Actual service name in docker-compose.yml is `gorp` | Change to `gorp` |
| 98 | `docker-compose exec matrix-bridge which claude` | Service is `gorp`, not `matrix-bridge` | Change to `gorp` |
| 103 | Container runs as "claude" user (UID 1000) | Dockerfile creates user `gorp`, not `claude` | Change to `gorp` |

### docs/HELP.md

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 138 | GitHub URL `https://github.com/2389-research/gorp` | Repo is `gorp-rs` | Change to `gorp-rs` |
| 139 | Issues URL `https://github.com/2389-research/gorp/issues` | Should be `gorp-rs` | Change to `gorp-rs` |

### docs/testing.md

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 14 | `tests/claude_tests.rs` | File does not exist | Remove reference or note actual test files |
| 34-39 | `.scratch/` directory with 5 test scripts | Entire `.scratch/` directory does not exist | Remove or mark as historical |
| 49 | `.scratch/run_all_scenarios.sh` | Does not exist | Remove |
| 14, 36, 43, 148-158, etc. | "sled database" (referenced ~15 times) | Project uses **SQLite (rusqlite)**, not sled | Replace all "sled" references with "SQLite" |
| 297 | "Tests fail with sled errors" | Should be SQLite errors | Update troubleshooting |

### docs/ACP-MIGRATION-STATUS.md

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 11 | `src/acp_client.rs` | Does not exist. ACP client is at `gorp-agent/src/backends/acp.rs` | Update path |
| 12 | `src/warm_session.rs` | Does not exist as standalone file. Warm session logic is distributed across multiple files | Update or remove |
| 110-123 | Test scripts in `.scratch/` | `.scratch/` directory does not exist | Remove or mark as archived |
| 131 | `src/webhook.rs - MCP endpoint at /mcp` | MCP endpoint at `/mcp` does exist in webhook.rs | No fix needed |

### config.toml.example

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 10 | `Sessions DB: <workspace>/channels.db` | Code uses `sessions.db` (gorp-core/src/session.rs:146) | Change to `sessions.db` |

### Dockerfile

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 5 | "Using Rust 1.85+ which has edition 2024 stabilized" | All Cargo.toml files use `edition = "2021"`, not 2024 | Update comment to reflect edition 2021 |

---

## Pattern Summary

| Pattern | Count | Root Cause |
|---------|-------|------------|
| Dead source file references | 5 | Architecture refactored into workspace crates (gorp-agent, gorp-core) but docs still reference flat src/ layout |
| Dead test file/script references | 11 | `.scratch/` directory and `tests/claude_tests.rs` deleted but docs not updated |
| Wrong database technology | ~15 instances | testing.md references "sled" everywhere but project migrated to SQLite/rusqlite |
| Wrong config section name | 3 instances | `[acp]` and `[claude]` sections renamed to `[backend]` but README.md, GET-STARTED.md not updated |
| Wrong Docker references | 5 instances | DOCKER.md references old service name, user name, volumes, env vars |
| Wrong env var names | 2 instances | `CLAUDE_BINARY_PATH` should be `BACKEND_BINARY` |
| Stale version number | 1 instance | MOTD.md says 0.2.1, actual is 0.3.2 |
| Wrong GitHub repo name | 2 instances | `gorp` should be `gorp-rs` |
| Wrong MCP config filename | 1 instance | `.mcp-servers.json` should be `.mcp.json` |

---

## Gap Detection (Pass 2B)

### Documented but not in code
- `!clear` command (GET-STARTED.md) - does not exist
- `.scratch/` test scripts - entire directory missing

### In code but not documented
- `!setup` command (onboarding wizard) - exists in matrix_commands.rs but not in HELP.md
- `!backend` command - switch backends at runtime, exists in commands.rs but not in HELP.md
- `gorp-ffi/` crate - exists in repo but not mentioned in CLAUDE.md architecture
- `src/claude_jail.rs` - exists but only mentioned in its own subdirectory README
- `src/admin/` module - admin web panel, partially documented in plans only
- `src/gui/` module - full desktop GUI, not documented in any user-facing docs
- `src/onboarding.rs` - onboarding flow, not documented
- Backend types: `direct`, `mock`, `direct_codex` - exist as backends but only partially documented in config.toml.example
- Multiple test files not mentioned in testing.md: `admin_routes_tests.rs`, `dedup_integration_tests.rs`, `dispatch_integration.rs`, `message_handler_tests.rs`, `onboarding_tests.rs`

---

## Human Review Queue

- [ ] README.md line 18-25: Are ACP adapters still the recommended install path, or should mux be the default?
- [ ] GET-STARTED.md: Full rewrite of config section needed to match actual [backend] structure
- [ ] docs/testing.md: Needs major rewrite - wrong database tech, missing test files, dead .scratch/ references
- [ ] docs/DOCKER.md: Needs substantial update for current Docker setup
- [ ] docs/ACP-MIGRATION-STATUS.md: Should be marked as historical/archived since architecture has evolved significantly
- [ ] docs/CHANGELOG.md: Last entry is 0.2.1 - missing changelog entries for 0.2.2 through 0.3.2
- [ ] Verify `@zed-industries/codex-acp` npm package still exists/is correct publisher

---

## Documents Verified

| Document | Claims | True | False | Accuracy |
|----------|--------|------|-------|----------|
| README.md | 25 | 17 | 8 | 68% |
| GET-STARTED.md | 18 | 14 | 4 | 78% |
| CLAUDE.md | 6 | 6 | 0 | 100% |
| docs/MOTD.md | 1 | 0 | 1 | 0% |
| docs/DOCKER.md | 15 | 9 | 6 | 60% |
| docs/HELP.md | 20 | 18 | 2 | 90% |
| docs/testing.md | 22 | 8 | 14 | 36% |
| docs/ACP-MIGRATION-STATUS.md | 12 | 8 | 4 | 67% |
| config.toml.example | 8 | 7 | 1 | 88% |
| Dockerfile | 3 | 2 | 1 | 67% |
| packaging/README.md | 8 | 8 | 0 | 100% |
| docs/examples/* | 4 | 4 | 0 | 100% |

---

## Priority Fixes

### Critical (blocks new users)
1. **GET-STARTED.md** - Config section `[claude]` must be changed to `[backend]`
2. **README.md** - Config section `[acp]` must be changed to `[backend]`
3. **docs/DOCKER.md** - Service name, user name, env vars, volume paths all wrong
4. **GET-STARTED.md** - Remove `!clear` (doesn't exist), replace with `!reset`

### High (misleading)
5. **docs/MOTD.md** - Version 0.2.1 should be 0.3.2
6. **README.md** - Code structure section lists 4 non-existent files
7. **docs/testing.md** - Entire sled/`.scratch/` narrative is false

### Medium (confusing but not blocking)
8. **docs/HELP.md** - GitHub URLs point to wrong repo
9. **README.md** - `.mcp-servers.json` should be `.mcp.json`
10. **config.toml.example** - `channels.db` comment should say `sessions.db`
11. **docs/ACP-MIGRATION-STATUS.md** - File paths outdated
