# ABOUTME: WebSocket server entrypoint for Claude Jail
# ABOUTME: Handles connections from gorp-rs and routes to SessionManager

"""WebSocket server entrypoint for Claude Jail."""

import asyncio
import json
import logging
import os
import signal
from typing import NoReturn

import websockets
from websockets import ServerConnection

from .protocol import CloseSessionMessage, QueryMessage, parse_inbound
from .session import SessionManager

logger = logging.getLogger(__name__)


class ClaudeJailServer:
    """WebSocket server for Claude Jail."""

    def __init__(
        self,
        host: str = "127.0.0.1",
        port: int = 31337,
        idle_timeout: int = 900,
    ):
        self.host = host
        self.port = port
        self.session_manager = SessionManager(idle_timeout_seconds=idle_timeout)
        self._server: websockets.WebSocketServer | None = None
        self._shutdown_event = asyncio.Event()

    async def start(self) -> None:
        """Start the WebSocket server."""
        await self.session_manager.start()

        self._server = await websockets.serve(
            self._handle_connection,
            self.host,
            self.port,
        )

        logger.info("Claude Jail listening on ws://%s:%d", self.host, self.port)

    async def stop(self) -> None:
        """Stop the WebSocket server."""
        if self._server:
            self._server.close()
            await self._server.wait_closed()

        await self.session_manager.stop()
        logger.info("Claude Jail stopped")

    async def _handle_connection(self, websocket: ServerConnection) -> None:
        """Handle a WebSocket connection from gorp-rs."""
        client_addr = websocket.remote_address
        logger.info("New connection from %s", client_addr)

        try:
            async for raw_message in websocket:
                await self._handle_message(websocket, raw_message)
        except websockets.exceptions.ConnectionClosed:
            logger.info("Connection closed from %s", client_addr)
        except Exception as e:
            logger.exception("Error handling connection from %s: %s", client_addr, e)

    async def _handle_message(
        self,
        websocket: ServerConnection,
        raw_message: str | bytes,
    ) -> None:
        """Handle a single message from gorp-rs."""
        try:
            if isinstance(raw_message, bytes):
                raw_message = raw_message.decode("utf-8")

            data = json.loads(raw_message)
            message = parse_inbound(data)

            if isinstance(message, QueryMessage):
                await self._handle_query(websocket, message)
            elif isinstance(message, CloseSessionMessage):
                await self._handle_close_session(message)
            else:
                logger.warning("Unknown message type: %s", type(message))

        except json.JSONDecodeError as e:
            logger.error("Invalid JSON: %s", e)
        except ValueError as e:
            logger.error("Invalid message: %s", e)

    async def _handle_query(
        self,
        websocket: ServerConnection,
        message: QueryMessage,
    ) -> None:
        """Handle a query message."""
        logger.info(
            "Query for channel %s: %s...",
            message.channel_id,
            message.prompt[:50] if len(message.prompt) > 50 else message.prompt,
        )

        async for response in self.session_manager.process_query(
            channel_id=message.channel_id,
            workspace=message.workspace,
            prompt=message.prompt,
            resume_id=message.session_id,
        ):
            await websocket.send(response.model_dump_json())

    async def _handle_close_session(self, message: CloseSessionMessage) -> None:
        """Handle a close session message."""
        logger.info("Closing session for channel %s", message.channel_id)
        await self.session_manager.close_session(message.channel_id)

    async def run_forever(self) -> NoReturn:
        """Run the server until shutdown signal."""
        await self.start()

        # Set up signal handlers
        loop = asyncio.get_running_loop()
        for sig in (signal.SIGTERM, signal.SIGINT):
            loop.add_signal_handler(sig, self._shutdown_event.set)

        await self._shutdown_event.wait()
        await self.stop()


def main() -> None:
    """Main entrypoint for Claude Jail."""
    # Configure logging
    log_level = os.environ.get("CLAUDE_JAIL_LOG_LEVEL", "INFO").upper()
    logging.basicConfig(
        level=getattr(logging, log_level),
        format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
    )

    # Get configuration from environment
    host = os.environ.get("CLAUDE_JAIL_HOST", "127.0.0.1")
    port = int(os.environ.get("CLAUDE_JAIL_PORT", "31337"))
    idle_timeout = int(os.environ.get("CLAUDE_JAIL_IDLE_TIMEOUT", "900"))

    logger.info("Starting Claude Jail...")
    logger.info("  Host: %s", host)
    logger.info("  Port: %d", port)
    logger.info("  Idle timeout: %ds", idle_timeout)

    server = ClaudeJailServer(
        host=host,
        port=port,
        idle_timeout=idle_timeout,
    )

    asyncio.run(server.run_forever())


if __name__ == "__main__":
    main()
