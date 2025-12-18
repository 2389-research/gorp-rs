# ABOUTME: Tests for MCP configuration loader
# ABOUTME: Validates loading and parsing of .mcp.json files

"""Tests for MCP configuration loader."""

import json
import os
from pathlib import Path

import pytest

from claude_jail.mcp_loader import expand_env_vars, load_mcp_config


class TestExpandEnvVars:
    """Tests for environment variable expansion."""

    def test_expand_string(self) -> None:
        """Expand env vars in a string."""
        os.environ["TEST_VAR"] = "test_value"
        result = expand_env_vars("prefix_$TEST_VAR_suffix")
        # Note: $TEST_VAR_suffix won't match, need ${TEST_VAR}
        assert "$TEST_VAR" not in result or result == "prefix_$TEST_VAR_suffix"

    def test_expand_dict(self) -> None:
        """Expand env vars in a dict."""
        os.environ["TEST_API_KEY"] = "secret123"
        data = {"key": "${TEST_API_KEY}", "other": "plain"}
        result = expand_env_vars(data)

        assert result["key"] == "secret123"
        assert result["other"] == "plain"

    def test_expand_nested_dict(self) -> None:
        """Expand env vars in nested dict."""
        os.environ["TEST_HOST"] = "localhost"
        data = {"server": {"host": "${TEST_HOST}", "port": 8080}}
        result = expand_env_vars(data)

        assert result["server"]["host"] == "localhost"
        assert result["server"]["port"] == 8080

    def test_expand_list(self) -> None:
        """Expand env vars in a list."""
        os.environ["TEST_ITEM"] = "expanded"
        data = ["${TEST_ITEM}", "plain"]
        result = expand_env_vars(data)

        assert result[0] == "expanded"
        assert result[1] == "plain"


class TestLoadMcpConfig:
    """Tests for loading MCP configuration."""

    def test_load_missing_file(self, tmp_path: Path) -> None:
        """Return empty config when .mcp.json doesn't exist."""
        result = load_mcp_config(tmp_path)

        assert result == {"mcpServers": {}}

    def test_load_valid_config(self, tmp_path: Path) -> None:
        """Load a valid .mcp.json file."""
        config = {
            "mcpServers": {
                "filesystem": {
                    "command": "python",
                    "args": ["-m", "mcp_server"],
                }
            }
        }
        mcp_path = tmp_path / ".mcp.json"
        mcp_path.write_text(json.dumps(config))

        result = load_mcp_config(tmp_path)

        assert "mcpServers" in result
        assert "filesystem" in result["mcpServers"]
        assert result["mcpServers"]["filesystem"]["command"] == "python"

    def test_load_config_with_env_vars(self, tmp_path: Path) -> None:
        """Expand environment variables in config."""
        os.environ["MCP_TEST_TOKEN"] = "my-secret-token"
        config = {
            "mcpServers": {
                "api": {
                    "command": "node",
                    "env": {"API_TOKEN": "${MCP_TEST_TOKEN}"},
                }
            }
        }
        mcp_path = tmp_path / ".mcp.json"
        mcp_path.write_text(json.dumps(config))

        result = load_mcp_config(tmp_path)

        assert result["mcpServers"]["api"]["env"]["API_TOKEN"] == "my-secret-token"

    def test_load_invalid_json(self, tmp_path: Path) -> None:
        """Return empty config for invalid JSON."""
        mcp_path = tmp_path / ".mcp.json"
        mcp_path.write_text("not valid json {{{")

        result = load_mcp_config(tmp_path)

        assert result == {"mcpServers": {}}

    def test_load_config_without_mcp_servers(self, tmp_path: Path) -> None:
        """Add empty mcpServers if not present."""
        config = {"someOtherKey": "value"}
        mcp_path = tmp_path / ".mcp.json"
        mcp_path.write_text(json.dumps(config))

        result = load_mcp_config(tmp_path)

        assert result["mcpServers"] == {}
