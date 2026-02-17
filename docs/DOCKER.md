# Running Matrix-Claude Bridge in Docker

## Quick Start

### 1. Build the image

```bash
docker-compose build
```

### 2. Configure environment variables

Make sure your `.env` file has the required variables:

```bash
MATRIX_HOME_SERVER=https://matrix.org
MATRIX_USER_ID=@yourbot:matrix.org
MATRIX_PASSWORD=your_password
ALLOWED_USERS=@youruser:matrix.org
BACKEND_BINARY=/usr/local/bin/claude
```

### 3. Run the container

```bash
docker-compose up -d
```

### 4. View logs

```bash
docker-compose logs -f
```

## Important: Claude CLI Access

The bot needs access to the Claude CLI binary. You have two options:

### Claude Code Installation

Claude Code is installed inside the container via npm during the Docker build.
The Dockerfile installs Node.js 22.x and then runs:

```dockerfile
npm install -g @anthropic-ai/claude-code
npm install -g @zed-industries/claude-code-acp
```

No host mount is needed for the Claude binary.

## Persistent Data

The following directories are persisted as volumes under `./app-data/`:
- `./app-data/config` - gorp configuration (config.toml)
- `./app-data/data` - crypto store, logs, scheduled prompts db
- `./app-data/workspace` - Claude session workspace directories
- `./app-data/claude-config` - Claude Code auth config
- `./app-data/claude-settings` - Claude CLI settings

## Useful Commands

```bash
# Stop the container
docker-compose down

# Rebuild after code changes
docker-compose up -d --build

# View container status
docker-compose ps

# Execute commands in container
docker-compose exec gorp /bin/bash

# Remove everything (including volumes)
docker-compose down -v
```

## Troubleshooting

### Container can't find claude binary

Check if claude is mounted correctly:
```bash
docker-compose exec gorp which claude
```

### Permission errors with volumes

The container runs as the `gorp` user (UID 1000). If you have permission issues:
```bash
sudo chown -R 1000:1000 app-data
```

### View detailed logs

```bash
docker-compose logs -f --tail=100
```
