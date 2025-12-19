# ABOUTME: Session management for Claude Jail
# ABOUTME: Manages per-channel session state with idle cleanup using query() API

"""Session management for Claude Jail."""

import asyncio
import logging
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import AsyncIterator

from claude_code_sdk import (
    AssistantMessage,
    ClaudeCodeOptions,
    ResultMessage,
    SystemMessage,
    TextBlock,
    ToolUseBlock,
    UserMessage,
    query,
)

from .mcp_loader import load_mcp_config
from .protocol import DoneMessage, ErrorMessage, OutboundMessage, TextMessage, ToolUseMessage

logger = logging.getLogger(__name__)


# Type alias for SDK message types
SdkMessage = UserMessage | AssistantMessage | SystemMessage | ResultMessage


@dataclass
class ChannelSession:
    """Session state for a specific channel."""

    channel_id: str
    workspace: Path
    last_activity: float = field(default_factory=time.time)
    session_id: str | None = None

    def touch(self) -> None:
        """Update last activity timestamp."""
        self.last_activity = time.time()

    async def close(self) -> None:
        """Clean up the session."""
        logger.info("Closed session for channel %s", self.channel_id)


class SessionManager:
    """Manages Claude sessions for multiple channels."""

    def __init__(self, idle_timeout_seconds: int = 900):
        self.sessions: dict[str, ChannelSession] = {}
        self.idle_timeout = idle_timeout_seconds
        self._cleanup_task: asyncio.Task | None = None

    async def start(self) -> None:
        """Start the session manager and cleanup task."""
        self._cleanup_task = asyncio.create_task(self._cleanup_loop())
        logger.info("Session manager started with %ds idle timeout", self.idle_timeout)

    async def stop(self) -> None:
        """Stop the session manager and close all sessions."""
        if self._cleanup_task:
            self._cleanup_task.cancel()
            try:
                await self._cleanup_task
            except asyncio.CancelledError:
                pass

        for session in list(self.sessions.values()):
            await session.close()
        self.sessions.clear()
        logger.info("Session manager stopped")

    async def _cleanup_loop(self) -> None:
        """Periodically clean up idle sessions."""
        while True:
            await asyncio.sleep(60)  # Check every minute
            await self._cleanup_idle_sessions()

    async def _cleanup_idle_sessions(self) -> None:
        """Close sessions that have been idle too long."""
        now = time.time()
        to_close = [
            channel_id
            for channel_id, session in self.sessions.items()
            if now - session.last_activity > self.idle_timeout
        ]

        for channel_id in to_close:
            session = self.sessions.pop(channel_id)
            await session.close()
            logger.info("Closed idle session for channel %s", channel_id)

    def get_or_create_session(
        self,
        channel_id: str,
        workspace: str,
        resume_id: str | None = None,
    ) -> ChannelSession:
        """Get existing session or create a new one."""
        # Return existing session if available
        if channel_id in self.sessions:
            session = self.sessions[channel_id]
            session.touch()
            logger.debug("Reusing existing session for channel %s", channel_id)
            return session

        workspace_path = Path(workspace)
        session = ChannelSession(
            channel_id=channel_id,
            workspace=workspace_path,
            session_id=resume_id,
        )

        self.sessions[channel_id] = session
        logger.info("Created new session for channel %s", channel_id)

        return session

    async def close_session(self, channel_id: str) -> None:
        """Explicitly close a session."""
        if channel_id in self.sessions:
            session = self.sessions.pop(channel_id)
            await session.close()

    async def process_query(
        self,
        query_id: str,
        channel_id: str,
        workspace: str,
        prompt: str,
        resume_id: str | None = None,
    ) -> AsyncIterator[OutboundMessage]:
        """
        Process a query and yield response messages.

        Uses the query() function which handles subprocess management correctly.

        Args:
            query_id: Unique ID for this query (echoed in all responses)
            channel_id: Channel/room ID for session management
            workspace: Path to workspace directory
            prompt: The user's prompt
            resume_id: Optional session ID for resumption

        Yields:
            OutboundMessage instances (TextMessage, ToolUseMessage, DoneMessage, ErrorMessage)
        """
        try:
            session = self.get_or_create_session(channel_id, workspace, resume_id)
            session.touch()

            # Load MCP config from workspace
            mcp_config = load_mcp_config(session.workspace)

            # Create options for the SDK query
            options = ClaudeCodeOptions(
                mcp_servers=mcp_config.get("mcpServers", {}),
                cwd=session.workspace,
                permission_mode="bypassPermissions",  # Trust gorp to manage permissions
                resume=session.session_id,
            )

            result_session_id = ""

            # Use query() function which handles subprocess correctly
            async for message in query(prompt=prompt, options=options):
                if isinstance(message, AssistantMessage):
                    for block in message.content:
                        if isinstance(block, TextBlock):
                            yield TextMessage(
                                query_id=query_id,
                                channel_id=channel_id,
                                content=block.text,
                            )
                        elif isinstance(block, ToolUseBlock):
                            yield ToolUseMessage(
                                query_id=query_id,
                                channel_id=channel_id,
                                tool=block.name,
                                input=block.input,
                            )
                elif isinstance(message, ResultMessage):
                    # Extract session ID for future resumption
                    if hasattr(message, "session_id") and message.session_id:
                        result_session_id = message.session_id
                        session.session_id = result_session_id

            yield DoneMessage(
                query_id=query_id,
                channel_id=channel_id,
                session_id=result_session_id or session.session_id or "",
            )

        except Exception as e:
            logger.exception("Error processing query for channel %s", channel_id)
            yield ErrorMessage(
                query_id=query_id,
                channel_id=channel_id,
                message=str(e),
            )
