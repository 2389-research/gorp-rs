# Create Command Any Context Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Allow `!create <name>` to work outside DMs while preserving its current behavior of creating a new workspace room and inviting the requesting user.

**Architecture:** Keep the change localized to the Matrix command handler by removing the DM-only guard for `create` and preserving the existing room creation flow. Update tests and user-facing copy so the command behavior and guidance match the new policy.

**Tech Stack:** Rust, Tokio, Matrix SDK, cargo test

---

### Task 1: Add failing tests for non-DM create behavior and copy

**Files:**
- Modify: `src/message_handler/commands.rs`
- Modify: `tests/message_handler_tests.rs`

**Step 1: Write the failing test**

Add a unit test in `src/message_handler/commands.rs` that asserts non-DM command help includes `!create <name> - Create new channel`.

Add or update a test in `tests/message_handler_tests.rs` that currently expects the DM-only rejection so it instead asserts the old rejection copy is no longer present for `!create`.

**Step 2: Run test to verify it fails**

Run: `cargo test create --quiet`

Expected: FAIL because current command help or behavior still reflects DM-only handling for `!create`.

**Step 3: Write minimal implementation**

Do not implement yet. This task stops after confirming the failure.

**Step 4: Run test to verify it still fails correctly**

Run: `cargo test create --quiet`

Expected: FAIL with assertions tied to the old DM-only behavior.

**Step 5: Commit**

Do not commit yet.

### Task 2: Remove DM-only gating for Matrix `!create`

**Files:**
- Modify: `src/message_handler/matrix_commands.rs`

**Step 1: Write the failing test**

Use the failing tests from Task 1 as the red state for this behavior change.

**Step 2: Run test to verify it fails**

Run: `cargo test create --quiet`

Expected: FAIL because `handle_matrix_command` still rejects `!create` when `is_dm` is `false`.

**Step 3: Write minimal implementation**

Remove only the `if !is_dm { ... return Ok(()); }` branch from the `create` command handler and keep the rest of the command flow unchanged.

**Step 4: Run test to verify it passes**

Run: `cargo test create --quiet`

Expected: PASS for the updated `!create` expectations.

**Step 5: Commit**

Do not commit yet.

### Task 3: Update user-facing copy that still claims DM-only creation

**Files:**
- Modify: `src/message_handler/mod.rs`
- Modify: `src/admin/routes.rs`
- Modify: `docs/HELP.md`
- Modify: `README.md`

**Step 1: Write the failing test**

Add or update a targeted assertion in an existing command/help test that checks the non-attached-room guidance no longer says `DM me to create one`.

**Step 2: Run test to verify it fails**

Run: `cargo test status --quiet`

Expected: FAIL because current guidance still instructs the user to DM the bot.

**Step 3: Write minimal implementation**

Update copy to say `Use !create <name>` or equivalent wording that does not require DMs for creation.

**Step 4: Run test to verify it passes**

Run: `cargo test status --quiet`

Expected: PASS for the updated copy assertions.

**Step 5: Commit**

Do not commit yet.

### Task 4: Run focused verification

**Files:**
- Modify: `src/message_handler/matrix_commands.rs`
- Modify: `src/message_handler/mod.rs`
- Modify: `src/message_handler/commands.rs`
- Modify: `tests/message_handler_tests.rs`
- Modify: `docs/HELP.md`
- Modify: `README.md`
- Modify: `src/admin/routes.rs`

**Step 1: Write the failing test**

No new test. Verification task only.

**Step 2: Run test to verify current state**

Run: `cargo test create --quiet`
Expected: PASS

Run: `cargo test status --quiet`
Expected: PASS

Run: `cargo test message_handler --quiet`
Expected: PASS

**Step 3: Write minimal implementation**

Only fix issues discovered by focused verification if needed.

**Step 4: Run test to verify it passes**

Run: `cargo test message_handler --quiet`

Expected: PASS

**Step 5: Commit**

```bash
git add src/message_handler/matrix_commands.rs src/message_handler/mod.rs src/message_handler/commands.rs tests/message_handler_tests.rs docs/HELP.md README.md src/admin/routes.rs docs/plans/2026-03-17-create-command-any-context-design.md docs/plans/2026-03-17-create-command-any-context.md
git commit -m "feat: allow create commands outside DMs"
```
