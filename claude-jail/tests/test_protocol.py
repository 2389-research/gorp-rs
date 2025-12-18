# ABOUTME: Tests for protocol message types
# ABOUTME: Validates serialization/deserialization of WebSocket messages

"""Tests for protocol message types."""

import pytest

from claude_jail.protocol import (
    CloseSessionMessage,
    DoneMessage,
    ErrorMessage,
    QueryMessage,
    TextMessage,
    ToolUseMessage,
    parse_inbound,
)


class TestInboundMessages:
    """Tests for parsing inbound messages."""

    def test_parse_query_message(self) -> None:
        """Parse a valid query message."""
        data = {
            "type": "query",
            "channel_id": "!abc123:matrix.org",
            "workspace": "/path/to/workspace",
            "prompt": "Hello Claude",
        }
        msg = parse_inbound(data)

        assert isinstance(msg, QueryMessage)
        assert msg.channel_id == "!abc123:matrix.org"
        assert msg.workspace == "/path/to/workspace"
        assert msg.prompt == "Hello Claude"
        assert msg.session_id is None

    def test_parse_query_message_with_session_id(self) -> None:
        """Parse a query message with session resumption."""
        data = {
            "type": "query",
            "channel_id": "!abc123:matrix.org",
            "workspace": "/path/to/workspace",
            "prompt": "Continue please",
            "session_id": "session-uuid-123",
        }
        msg = parse_inbound(data)

        assert isinstance(msg, QueryMessage)
        assert msg.session_id == "session-uuid-123"

    def test_parse_close_session_message(self) -> None:
        """Parse a close session message."""
        data = {
            "type": "close_session",
            "channel_id": "!abc123:matrix.org",
        }
        msg = parse_inbound(data)

        assert isinstance(msg, CloseSessionMessage)
        assert msg.channel_id == "!abc123:matrix.org"

    def test_parse_unknown_type_raises(self) -> None:
        """Unknown message type raises ValueError."""
        data = {"type": "unknown", "foo": "bar"}

        with pytest.raises(ValueError, match="Unknown message type"):
            parse_inbound(data)


class TestOutboundMessages:
    """Tests for outbound message serialization."""

    def test_text_message_json(self) -> None:
        """TextMessage serializes correctly."""
        msg = TextMessage(channel_id="!abc:test", content="Hello world")
        data = msg.model_dump()

        assert data == {
            "type": "text",
            "channel_id": "!abc:test",
            "content": "Hello world",
        }

    def test_tool_use_message_json(self) -> None:
        """ToolUseMessage serializes correctly."""
        msg = ToolUseMessage(
            channel_id="!abc:test",
            tool="mcp__matrix__send_attachment",
            input={"file": "chart.png", "room": "!abc:test"},
        )
        data = msg.model_dump()

        assert data == {
            "type": "tool_use",
            "channel_id": "!abc:test",
            "tool": "mcp__matrix__send_attachment",
            "input": {"file": "chart.png", "room": "!abc:test"},
        }

    def test_done_message_json(self) -> None:
        """DoneMessage serializes correctly."""
        msg = DoneMessage(channel_id="!abc:test", session_id="uuid-123")
        data = msg.model_dump()

        assert data == {
            "type": "done",
            "channel_id": "!abc:test",
            "session_id": "uuid-123",
        }

    def test_error_message_json(self) -> None:
        """ErrorMessage serializes correctly."""
        msg = ErrorMessage(channel_id="!abc:test", message="Something went wrong")
        data = msg.model_dump()

        assert data == {
            "type": "error",
            "channel_id": "!abc:test",
            "message": "Something went wrong",
        }
