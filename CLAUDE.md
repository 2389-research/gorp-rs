# gorp-rs

Matrix-to-Claude bridge with pluggable agent backends.

## Deployment

### Remote Access
- **tmux session**: `gorp-debugging` - SSH'd into gorp server for debugging/sysadmin
- **Remote path**: `~/docker/gorp` - contains multitenant docker setup
- **Compose file**: `docker-compose.multi.yml`

### Deploy Commands (via tmux)
```bash
# Send commands to the gorp-debugging tmux session:
tmux send-keys -t gorp-debugging "cd ~/docker/gorp && git pull" Enter
tmux send-keys -t gorp-debugging "DOCKER_BUILDKIT=1 docker compose -f docker-compose.multi.yml build --ssh default gorp-1" Enter
tmux send-keys -t gorp-debugging "docker compose -f docker-compose.multi.yml up -d gorp-1" Enter
```

### SSH Agent Forwarding
BuildKit needs SSH agent for private git dependencies (mux-rs). If SSH fails:
1. Re-establish SSH connection with `-A` flag
2. Verify with `ssh-add -l` in the tmux session

## Architecture

- `gorp-agent/` - Pluggable agent backend abstraction
  - `backends/acp.rs` - Claude Code CLI backend (agent-client-protocol)
  - `backends/mux.rs` - Native Rust backend using mux-rs
- `gorp-core/` - Shared core library (config, session store, paths, metrics)
- `src/` - Main gorp application (Matrix client, scheduler, web UI)
