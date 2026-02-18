# Documentation Audit Report

Generated: 2026-02-17 | Branch: `main` | Commit: `842a481`

## Executive Summary

| Metric | Count |
|--------|-------|
| Documents scanned | 17 |
| Claims verified | ~245 |
| Verified TRUE | ~189 (77%) |
| **Verified FALSE** | **56 (23%)** |

Post-merge audit of the `message-bus-refactor` branch. Prior audit (2026-02-16) fixed 49 claims; this audit found 56 new or residual issues, primarily in status/historical docs that were not covered by the previous audit.

## False Claims Requiring Fixes

### README.md (5 false)

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 18-25 | ACP adapter listed as prerequisite; Codex described as "default, faster responses" | Default backend is `acp` but binary is `None`; `mux` backend needs no npm install | Note ACP adapter only needed for `backend.type = "acp"`, mention `mux` alternative |
| 17 | "Claude Code CLI installed" as prerequisite | Only needed for ACP/direct backends, not `mux` | Add "if using ACP backend" qualifier |
| 51 | Example config shows `type = "acp"` | `config.toml.example` ships with `type = "mux"` | Align with shipped example |
| 142 | `backend.binary` described as "ACP agent binary" | Used by both ACP and direct backends | Change to "Agent binary path (used by acp and direct backends)" |
| 162 | "Check `crypto_store/` exists" | Actual path is `~/.local/share/gorp/crypto_store/` | Add full path |

### GET-STARTED.md (5 false)

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 152-153 | `gorp start` as binary invocation | No standalone install method described | Change to `cargo run --release -- start` |
| 189 | `!join roomname` described as "Create/join a topic room" | `!join` only invites to existing channel | Change to "Get invited to an existing channel room" |
| 190 | `!schedule every day at 9am: Good morning!` | Colon syntax not used in parser | Remove the colon |
| 192 | `!reset` described as "Reset conversation history" | Resets entire Claude session, not just history | Change to "Reset Claude session (starts fresh)" |
| 204 | "Check that gorp is running: `gorp start`" | `gorp start` starts gorp, doesn't check status | Rewrite troubleshooting instruction |

### docs/DOCKER.md (4 false)

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 20 | `BACKEND_BINARY` listed as required env var | It's optional; binary found on PATH via npm | Move to optional section |
| 55 | `./app-data/data` contains "scheduled prompts db" | Scheduled prompts are in `sessions.db` in workspace dir | Change to "crypto store, logs" |
| 83-86 | "Check if claude is mounted correctly" | Claude is installed via npm, not mounted | Change "mounted" to "installed" |
| 90 | "runs as the `gorp` user (UID 1000)" | UID not explicitly set in Dockerfile | Remove UID claim or add `--uid 1000` to Dockerfile |

### docs/HELP.md (3 false)

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 17-23 | DM Commands missing `!reset <name>` | Code's DM help lists `!reset <name>` as DM command | Add `!reset <name>` to DM Commands section |
| 29-35 | Lists `!changelog` and `!motd` as room commands | These work but aren't shown in `!help` output | Add note that these are supplementary commands |
| 118-127 | `.mcp.json` and `.claude/settings.json` listed as always present | Only present if copied from `workspace/template/` | Add note about template dependency |

### docs/testing.md (5 false)

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 59 | `INFO Starting Matrix-Claude Bridge` | Actual: `INFO Starting gorp - Matrix-Claude Bridge` | Update log message |
| 63 | `INFO Joined room` | No such log message in codebase | Remove or replace with actual log message |
| 64 | `INFO Message handler registered, starting sync loop` | Actual: `INFO Starting continuous sync loop with LocalSet` | Update log message |
| 123-125 | "Bot logs decryption failure" for unverified device | No explicit decryption failure handling in gorp | Note this depends on Matrix SDK internals |
| 217 | "Tests fallback to `cargo test` automatically" | No such mechanism exists | Remove or rephrase |

### docs/ACP-MIGRATION-STATUS.md (13 false)

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 11 | `src/acp_client.rs` | Actual: `gorp-agent/src/backends/acp.rs` | Fix path |
| 12 | `src/warm_session.rs` | Actual: `gorp-core/src/warm_session.rs` | Fix path |
| 13 | "Messages flow Matrix -> ACP -> AI -> Matrix" | Now uses message bus architecture | Rewrite flow description |
| 19-23 | Config section `[acp]` with `agent_binary` | Actual: `[backend]` with `type`, `binary`, `timeout_secs` | Fix config block |
| 20 | Default backend "codex-acp" | Default is `"acp"` | Fix default |
| 46 | `src/message_handler.rs` (single file) | Module directory at `src/message_handler/` | Fix path |
| 57-58 | `AcpClient` struct | Actual: `AcpBackend`, `PersistentAcpClient` in gorp-agent | Fix struct names |
| 106 | `load_session` not supported | IS supported in ACP backend | Clarify behavior |
| 110-117 | Test scripts in `.scratch/` | None of those exist | Remove or update |
| 119-122 | `cargo run --bin test_rapid_prompts` | No such binary target | Remove |
| 127-132 | File reference section | Multiple wrong paths | Fix all paths |

### docs/ENCRYPTION-SUCCESS-REPORT.md (4 false)

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 5 | "Matrix SDK Version: 0.7" | Current: 0.16 | Update version |
| 98 | Code at `src/main.rs:268-279` | Moved to `src/server.rs:166-177` | Update path |
| 100-113 | Tracing message text | Messages changed | Update snippet |
| 121 | Auto-join handler at `src/main.rs:101-126` | Now at `src/main.rs:1757-1807` | Update line numbers |

### docs/MESSAGE-DISPLAY-BUG.md (2 false)

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 2 | `message_handler.rs:441-443` | Module directory, streaming changed | Update reference |
| 187 | `src/message_handler.rs` | Actual: `src/message_handler/mod.rs` | Fix path |

### docs/SESSION-RESUME-LIMITATION.md (5 false)

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 77-85 | `acp_client.load_session()` | Actual: `agent_handle.load_session()` | Update code snippet |
| 182-188 | scenarios.jsonl "Update needed" | Already updated | Remove section |
| 192 | `src/warm_session.rs` | Actual: `gorp-core/src/warm_session.rs` | Fix path |
| 193 | `src/acp_client.rs` | Actual: `gorp-agent/src/backends/acp.rs` | Fix path |
| 194 | `src/session.rs` | Actual: `gorp-core/src/session.rs` | Fix path |

### docs/USER-STORY.md (1 false)

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 236-243 | `gorp schedule list` output format | Actual columns: ID, Status, Next Execution, Prompt | Update example output |

### docs/CHANGELOG.md (2 false)

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 20 | `!changelog` command exists | No `!changelog` in codebase | Remove reference |
| 37 | `!reset` "reloads MCP tools" | Just resets session, no MCP reload | Fix description |

### docs/examples/matrix-bridge/README.md (1 false)

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 13 | "Pipes replies as Markdown/plain text" | Sends plain text only, no formatted_body | Change to "plain text" |

### docs/examples/simple-tui/README.md (1 false)

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 18-19 | `npm install` + `npm start` works | chalk v5 is ESM-only, CJS require() will crash | Downgrade chalk to ^4.1.2 or convert to ESM |

### claude-jail/README.md (3 false)

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 1-2 | ABOUTME: "sandboxed execution", "security policies" | WebSocket server wrapping Claude Agent SDK | Rewrite ABOUTME |
| 6-8 | Body: "security policies and resource constraints" | Python service with `bypassPermissions` mode | Rewrite description |

### packaging/README.md (2 false)

| Line | Claim | Reality | Fix |
|------|-------|---------|-----|
| 26 | "needs Python/Pillow" | Only needs Pillow as fallback; primary tool is sips/iconutil | Clarify fallback |
| 70 | `brew install --cask gorp` | Formula, not Cask; correct: `brew install gorp` | Remove `--cask` |

### config.toml.example (unfixed from prior audit)

| File | Line | Claim | Reality | Fix |
|------|------|-------|---------|-----|
| `config.toml.example` | 10 | `channels.db` | Code uses `sessions.db` | Fix filename |
| `example/config.toml.example` | 10 | `channels.db` | Code uses `sessions.db` | Fix filename |

## Pattern Summary

| Pattern | Count | Root Cause |
|---------|-------|------------|
| Wrong file paths (pre-workspace restructure) | 15 | Files moved to `gorp-core/` or `gorp-agent/` subcrates |
| Outdated config section (`[acp]` → `[backend]`) | 4 | Config renamed, status docs not updated |
| Stale code snippets/line numbers | 6 | Code evolved, embedded snippets not updated |
| Outdated behavioral descriptions | 8 | Message bus architecture, session handling changed |
| Missing/phantom test scripts | 3 | Scripts documented but never created |
| Inaccurate log messages | 3 | Log messages changed, testing.md not updated |
| `channels.db` → `sessions.db` | 2 | Unfixed from 2026-02-11 audit |
| Miscellaneous (Homebrew cask, chalk ESM, etc.) | 7 | Various |

## Human Review Queue

- [ ] simple-tui chalk v5 ESM/CJS incompatibility — code fix needed, not just doc fix
- [ ] ACP-MIGRATION-STATUS.md — document is heavily outdated; consider archiving
- [ ] ENCRYPTION-SUCCESS-REPORT.md — embedded code snippets with line numbers are maintenance burden
- [ ] MESSAGE-DISPLAY-BUG.md — describes old chunking architecture that may have changed
