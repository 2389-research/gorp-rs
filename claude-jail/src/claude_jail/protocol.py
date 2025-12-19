# ABOUTME: WebSocket protocol message types for Claude Jail
# ABOUTME: Pydantic models for gorp-rs <-> Claude Jail communication

"""WebSocket protocol message types for Claude Jail."""

from typing import Any, Literal

from pydantic import BaseModel


# --- Inbound messages (gorp -> Claude Jail) ---


class QueryMessage(BaseModel):
    """Request to start or continue a conversation."""

    type: Literal["query"] = "query"
    query_id: str  # Unique ID for this query (prevents race conditions)
    channel_id: str
    workspace: str
    prompt: str
    session_id: str | None = None


class CloseSessionMessage(BaseModel):
    """Request to explicitly close a session."""

    type: Literal["close_session"] = "close_session"
    channel_id: str


InboundMessage = QueryMessage | CloseSessionMessage


# --- Outbound messages (Claude Jail -> gorp) ---


class TextMessage(BaseModel):
    """Streaming text chunk from Claude."""

    type: Literal["text"] = "text"
    query_id: str  # Echo back the query_id
    channel_id: str
    content: str


class ToolUseMessage(BaseModel):
    """Notification that Claude is calling an MCP tool."""

    type: Literal["tool_use"] = "tool_use"
    query_id: str  # Echo back the query_id
    channel_id: str
    tool: str
    input: dict[str, Any]


class DoneMessage(BaseModel):
    """Conversation complete."""

    type: Literal["done"] = "done"
    query_id: str  # Echo back the query_id
    channel_id: str
    session_id: str


class ErrorMessage(BaseModel):
    """Error occurred during processing."""

    type: Literal["error"] = "error"
    query_id: str  # Echo back the query_id
    channel_id: str
    message: str


OutboundMessage = TextMessage | ToolUseMessage | DoneMessage | ErrorMessage


def parse_inbound(data: dict[str, Any]) -> InboundMessage:
    """Parse a JSON dict into an inbound message."""
    msg_type = data.get("type")
    if msg_type == "query":
        return QueryMessage.model_validate(data)
    elif msg_type == "close_session":
        return CloseSessionMessage.model_validate(data)
    else:
        raise ValueError(f"Unknown message type: {msg_type}")
