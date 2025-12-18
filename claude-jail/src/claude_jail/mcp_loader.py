# ABOUTME: MCP configuration loader for Claude Jail
# ABOUTME: Loads .mcp.json files from workspace directories

"""MCP configuration loader for Claude Jail."""

import json
import logging
import os
from pathlib import Path
from typing import Any

logger = logging.getLogger(__name__)


def expand_env_vars(value: Any) -> Any:
    """Recursively expand environment variables in config values."""
    if isinstance(value, str):
        return os.path.expandvars(value)
    elif isinstance(value, dict):
        return {k: expand_env_vars(v) for k, v in value.items()}
    elif isinstance(value, list):
        return [expand_env_vars(v) for v in value]
    return value


def load_mcp_config(workspace: str | Path) -> dict[str, Any]:
    """
    Load MCP server configuration from workspace .mcp.json.

    Args:
        workspace: Path to the workspace directory

    Returns:
        Dict with "mcpServers" key containing server configurations.
        Returns empty dict if no .mcp.json found.
    """
    workspace_path = Path(workspace)
    mcp_path = workspace_path / ".mcp.json"

    if not mcp_path.exists():
        logger.debug("No .mcp.json found at %s", mcp_path)
        return {"mcpServers": {}}

    try:
        with open(mcp_path) as f:
            config = json.load(f)

        # Expand environment variables in the config
        config = expand_env_vars(config)

        # Ensure mcpServers key exists
        if "mcpServers" not in config:
            config["mcpServers"] = {}

        logger.info(
            "Loaded %d MCP servers from %s",
            len(config["mcpServers"]),
            mcp_path,
        )
        return config

    except json.JSONDecodeError as e:
        logger.error("Failed to parse %s: %s", mcp_path, e)
        return {"mcpServers": {}}
    except Exception as e:
        logger.error("Failed to load %s: %s", mcp_path, e)
        return {"mcpServers": {}}
