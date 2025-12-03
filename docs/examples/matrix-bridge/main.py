#!/usr/bin/env python3
"""
Matrix ↔ Claude Code bridge.

Listens for messages starting with !claude and proxies them to the Claude CLI.
"""
from __future__ import annotations

import asyncio
import json
import os
import sys
import uuid
from dataclasses import dataclass
from typing import Dict, Optional

from dotenv import load_dotenv
from nio import (
    AsyncClient,
    LoginResponse,
    RoomMessageText,
)

# Load environment variables from .env file
load_dotenv()

COMMAND_PREFIX = "!claude"
CLAUDE_BIN = os.environ.get("CLAUDE_BIN", "claude")
SDK_URL = os.environ.get("SDK_URL")


@dataclass
class SessionState:
    session_id: str
    started: bool = False

    def cli_args(self) -> list[str]:
        if self.started:
            return ["--resume", self.session_id]
        self.started = True
        return ["--session-id", self.session_id]

    def reset(self) -> None:
        self.session_id = str(uuid.uuid4())
        self.started = False


sessions: Dict[str, SessionState] = {}


def env_or_exit(name: str) -> str:
    value = os.environ.get(name)
    if not value:
        print(f"Missing required environment variable: {name}", file=sys.stderr)
        sys.exit(1)
    return value


async def run_claude(prompt: str, state: SessionState) -> str:
    args = [CLAUDE_BIN, "--print", "--output-format", "json", *state.cli_args()]
    if SDK_URL:
        args += ["--sdk-url", SDK_URL]
    args.append(prompt)

    # Log the command being run (hide the full prompt for brevity)
    args_display = args[:-1] + [f'"{prompt[:30]}..."']
    print(f"  💻 Running: {' '.join(args_display)}")

    proc = await asyncio.create_subprocess_exec(
        *args,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
    )
    stdout, stderr = await proc.communicate()
    stdout_text = stdout.decode("utf-8", errors="ignore").strip()
    stderr_text = stderr.decode("utf-8", errors="ignore").strip()

    if proc.returncode != 0:
        print(f"  ⚠️ Claude CLI failed with exit code {proc.returncode}")
        raise RuntimeError(
            f"Claude CLI exited with {proc.returncode}\n{stderr_text or stdout_text}"
        )

    if not stdout_text:
        return "(no output)"

    try:
        payload = json.loads(stdout_text)
        parts = [block.get("text", "") for block in payload.get("content", [])]
        text = "\n".join(parts).strip()
        return text or stdout_text
    except json.JSONDecodeError:
        return stdout_text


async def handle_message(room, event, client: AsyncClient, allowed_room_id: Optional[str]):
    # If a specific room_id is configured, only respond in that room
    if allowed_room_id and room.room_id != allowed_room_id:
        return
    if not isinstance(event, RoomMessageText):
        return
    if event.sender == client.user:
        return

    body = event.body.strip()
    if not body.lower().startswith(COMMAND_PREFIX):
        return

    current_room_id = room.room_id
    print(f"📨 Received message in {current_room_id} from {event.sender}: {body}")

    content = body[len(COMMAND_PREFIX) :].strip()
    if not content:
        await send_message(client, current_room_id, "Usage: !claude <prompt>")
        return

    session = sessions.setdefault(current_room_id, SessionState(str(uuid.uuid4())))
    print(f"🔑 Using session: {session.session_id[:8]}... (started: {session.started})")

    if content in {"/reset", "/restart"}:
        print(f"🔄 Resetting session...")
        session.reset()
        sessions[current_room_id] = session
        await send_message(
            client, current_room_id, f"🔄 Session reset (new id: {session.session_id})"
        )
        return
    if content in {"/end", "/stop"}:
        print(f"✂️ Ending session...")
        sessions.pop(current_room_id, None)
        await send_message(client, current_room_id, "✂️ Session ended.")
        return

    print(f"🤖 Invoking Claude CLI with prompt: {content[:50]}...")
    await send_typing(client, current_room_id, timeout=30000)
    try:
        response = await run_claude(content, session)
        print(f"✅ Claude responded ({len(response)} chars)")
    except Exception as exc:  # pylint: disable=broad-except
        print(f"❌ Claude error: {exc}")
        await send_message(client, current_room_id, f"⚠️ Claude error:\n{exc}")
        return
    finally:
        await send_typing(client, current_room_id, timeout=0)

    print(f"📤 Sending response to Matrix...")
    await send_message(client, current_room_id, response)
    print(f"✨ Done!")


async def send_message(client: AsyncClient, room_id: str, text: str) -> None:
    await client.room_send(
        room_id,
        message_type="m.room.message",
        content={
            "msgtype": "m.text",
            "body": text,
        },
    )


async def send_typing(client: AsyncClient, room_id: str, timeout: int) -> None:
    try:
        await client.room_typing(room_id, bool(timeout), timeout=timeout)
    except Exception:
        pass


async def login_client(client: AsyncClient, password: Optional[str]) -> None:
    if client.access_token:
        print("🔑 Using provided access token.")
        return
    if not password:
        print("❌ Password or access token required.", file=sys.stderr)
        sys.exit(1)
    print("🔐 Logging in to Matrix...")
    resp = await client.login(password=password, device_name="claude-matrix-bridge")
    if isinstance(resp, LoginResponse):
        print(f"✅ Logged in as {client.user_id}")
    else:
        print(f"❌ Login failed: {resp}", file=sys.stderr)
        sys.exit(1)


async def main() -> None:
    print("=" * 60)
    print("🌉 Matrix ↔ Claude Code Bridge")
    print("=" * 60)

    homeserver = env_or_exit("MATRIX_HOMESERVER")
    user_id = env_or_exit("MATRIX_USER")
    room_id = os.environ.get("MATRIX_ROOM_ID")  # Optional now!
    password = os.environ.get("MATRIX_PASSWORD")
    access_token = os.environ.get("MATRIX_ACCESS_TOKEN")

    print(f"🏠 Homeserver: {homeserver}")
    print(f"👤 Bot user: {user_id}")
    if room_id:
        print(f"💬 Restricted to room: {room_id}")
    else:
        print(f"💬 Listening in ALL rooms")
    print(f"🤖 Claude binary: {CLAUDE_BIN}")
    if SDK_URL:
        print(f"🔗 SDK URL: {SDK_URL}")
    print()

    client = AsyncClient(homeserver, user_id)
    if access_token:
        client.access_token = access_token
        client.user_id = user_id

    await login_client(client, password)
    client.add_event_callback(
        lambda room, event: handle_message(room, event, client, room_id),
        RoomMessageText,
    )

    print()
    print("=" * 60)
    print(f"👂 Listening for '{COMMAND_PREFIX}' commands...")
    print("=" * 60)
    print()
    await client.sync_forever(timeout=30000, full_state=True)


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        print("\nGoodbye!")
