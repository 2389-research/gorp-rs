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
CLAUDE_BINARY_PATH=/usr/local/bin/claude
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

### Option 1: Mount Claude from host (Recommended)

Uncomment this line in `docker-compose.yml`:

```yaml
volumes:
  - /usr/local/bin/claude:/usr/local/bin/claude:ro
```

Replace `/usr/local/bin/claude` with your actual claude binary path.

Find your claude path:
```bash
which claude
```

### Option 2: Install Claude in the container

Add this to the Dockerfile after the `FROM debian:bookworm-slim` line:

```dockerfile
# Install Node.js (if claude needs it)
RUN apt-get update && apt-get install -y curl && \
    curl -fsSL https://deb.nodesource.com/setup_20.x | bash - && \
    apt-get install -y nodejs && \
    npm install -g @anthropic-ai/claude-cli
```

## Persistent Data

The following directories are persisted as volumes:
- `./sessions_db` - Claude session data
- `./crypto_store` - Matrix encryption keys

## Useful Commands

```bash
# Stop the container
docker-compose down

# Rebuild after code changes
docker-compose up -d --build

# View container status
docker-compose ps

# Execute commands in container
docker-compose exec matrix-bridge /bin/bash

# Remove everything (including volumes)
docker-compose down -v
```

## Troubleshooting

### Container can't find claude binary

Check if claude is mounted correctly:
```bash
docker-compose exec matrix-bridge which claude
```

### Permission errors with volumes

The container runs as the `claude` user (UID 1000). If you have permission issues:
```bash
sudo chown -R 1000:1000 sessions_db crypto_store
```

### View detailed logs

```bash
docker-compose logs -f --tail=100
```
