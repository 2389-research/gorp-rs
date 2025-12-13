# gorp Help

gorp is a Matrix-Claude bridge that creates persistent AI assistant channels.

## Quick Start

1. DM the bot: `!create mychannel`
2. Join the created room
3. Start chatting with Claude!

## Commands

### DM Commands (Orchestrator)

These commands work in direct messages to the bot:

- `!create <name>` - Create a new channel with workspace
- `!join <name>` - Get invited to an existing channel
- `!delete <name>` - Remove channel (keeps workspace files)
- `!cleanup` - Leave orphaned rooms
- `!restore-rooms` - Restore channels from workspace directories
- `!list` - Show all your channels
- `!help` - Show this help

### Room Commands

These commands work in channel rooms:

- `!help` - Show this help
- `!status` - Show channel info (session, directory, debug state)
- `!debug on/off` - Toggle tool usage display
- `!reset` - Reset Claude session (reloads MCP tools)
- `!leave` - Bot leaves room (preserves workspace)
- `!changelog` - Show recent changes
- `!motd` - Show message of the day

### Scheduling Commands

Schedule prompts to run automatically:

- `!schedule <time> <prompt>` - Create a scheduled prompt
- `!schedule list` - View all scheduled prompts
- `!schedule delete <id>` - Remove a schedule
- `!schedule pause <id>` - Pause a schedule
- `!schedule resume <id>` - Resume a paused schedule
- `!schedule export` - Export schedules to `.gorp/schedule.yaml`
- `!schedule import` - Import schedules from `.gorp/schedule.yaml`

**Time formats:**
- Relative: `in 2 hours`, `in 30 minutes`, `tomorrow 9am`
- Natural: `every monday 8am`, `every day at noon`
- Cron: `0 8 * * MON` (8am every Monday)

**Examples:**
```
!schedule in 2 hours check my inbox
!schedule tomorrow 9am summarize my calendar
!schedule every monday 8am weekly standup reminder
```

## Features

- **Persistent Sessions**: Conversations continue across restarts
- **Workspace Directories**: Each channel has its own working directory
- **MCP Integration**: Full Claude Code capabilities with MCP tools
- **Webhooks**: Trigger prompts via HTTP POST
- **Scheduling**: One-time and recurring scheduled prompts
- **Debug Mode**: See what tools Claude is using

## Webhooks

Each channel has a webhook URL for external triggers:

```bash
POST http://localhost:13000/webhook/session/<session-id>
Content-Type: application/json

{"prompt": "Your message here"}
```

Get your session ID with `!status`.

## Workspace Structure

Each channel creates:
```
workspace/<channel>/
├── CLAUDE.md           # Channel instructions
├── .mcp.json           # MCP server config
├── .claude/            # Claude settings
│   └── settings.json   # Hooks configuration
└── .gorp/              # gorp data
    ├── context.json    # Session context
    └── schedule.yaml   # Exported schedules
```

## Tips

- Use `!debug on` to see what tools Claude is using
- Use `!reset` if MCP tools aren't working (reloads configuration)
- Export schedules before major changes with `!schedule export`
- Workspace files persist even after `!delete`

## More Info

- GitHub: https://github.com/2389-research/gorp
- Issues: https://github.com/2389-research/gorp/issues
