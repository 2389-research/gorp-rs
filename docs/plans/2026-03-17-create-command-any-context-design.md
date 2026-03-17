# Create Command Any Context Design

**Goal:** Allow `!create <name>` to be invoked outside DMs while preserving its existing behavior of creating a brand-new workspace room and inviting the requesting user.

## Behavior

`!create <name>` should work from any joined room the bot can access, including rooms that are already attached to an existing workspace.

The command keeps its current semantics:

- Validate the requested channel name
- Create a new Matrix room for the new workspace
- Invite the requesting user to that new room
- Persist the channel against the newly created room ID
- Reply in the room where the command was issued with creation details

This means invoking `!create` from a workspace room uses that room as a control surface only. It does not bind the current room to the new channel.

## Error Handling

Validation and duplicate-name handling stay unchanged.

If Matrix room creation or invitation fails, the command should continue to report the existing failure paths. The main user-facing change is that `!create` should no longer claim it only works in DMs.

## Testing

Add or update tests to cover:

- `!create` is allowed when `is_dm` is `false`
- Existing validation still applies outside DMs
- User-facing command guidance no longer incorrectly says users must DM the bot to create channels

## Scope

The behavior change is primarily in the Matrix command handler. Other command restrictions such as `!list`, `!join`, and `!delete` remain DM-only.
