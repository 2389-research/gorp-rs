# Changelog

All notable changes to gorp are documented here.

## [0.2.1] - 2025-12-12

### Added
- **Multi-instance support**: Run multiple gorp instances with separate data
  - `scripts/setup-instance.sh` - Interactive setup for new instances
  - `scripts/gorp-multi.sh` - Management script (start/stop/restart/update/status)
  - `docker-compose.multi.yml` - Pre-configured for 10 instances
- **Schedule export/import**: Backup and restore schedules via YAML
  - `!schedule export` - Export to `.gorp/schedule.yaml`
  - `!schedule import` - Import from `.gorp/schedule.yaml`
- **Workspace templates**: New instances get pre-configured workspace
  - CLAUDE.md with Matrix chat guidelines
  - .claude/settings.json with session hooks
  - .gorp/schedule.yaml with example schedules
- **MCP tools in Docker**: chronicle, memory, toki, pagen pre-installed
- **Help system**: `!help` and `!changelog` commands with docs

### Changed
- Renamed `.matrix` directory to `.gorp` throughout codebase
- Consolidated Docker volumes under `./app-data/`
- Improved YAML export to handle special characters safely

### Fixed
- Missing `[claude]` section in generated config.toml
- Port display bug for instance numbers > 9
- Input validation in setup scripts (Matrix ID format, room prefix)
- File permissions (600) for config files with credentials

## [0.2.0] - 2025-12-11

### Added
- **`!reset` command**: Reset Claude session and reload MCP tools
- **`set_room_avatar` MCP tool**: Set room avatars programmatically
- **`gorp rooms sync` command**: Rename all rooms to match prefix
- **Auto-rename rooms**: Rooms update when `room_prefix` config changes

### Changed
- Upgraded axum 0.7 → 0.8, askama 0.12 → 0.14
- Upgraded matrix-sdk 0.7 → 0.16 for authenticated media support

## [0.1.0] - 2025-12-10

### Added
- **Admin panel** at `/admin` with:
  - Dashboard with channel overview
  - Channel management and log viewer
  - Schedule management UI
  - Workspace file browser with markdown rendering
  - Health monitoring and error alerts
  - Search across all workspaces
- **Scheduling system**:
  - One-time and recurring prompts
  - Natural language time parsing
  - Cron expression support
  - Pause/resume functionality
- **Claude Code integration**: Full Claude Code CLI in Docker image
- **MCP server**: `/mcp` endpoint for Claude integration
- **Webhook support**: Trigger prompts via HTTP POST

### Initial Features
- Matrix bot with persistent Claude sessions
- Channel-based conversations with dedicated workspaces
- Debug mode for tool usage visibility
- DM orchestration for channel management
