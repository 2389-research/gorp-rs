#!/usr/bin/env python3
"""Minimal Python TUI that proxies to the Claude CLI."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import uuid


CLAUDE_BIN = os.environ.get("CLAUDE_BIN", "claude")
SDK_URL = os.environ.get("SDK_URL")


def clear_screen() -> None:
    os.system("cls" if os.name == "nt" else "clear")


class Session:
    def __init__(self) -> None:
        self.session_id = self._initial_session_id()
        self.started = False
        self.history: list[tuple[str, str]] = []

    def reset(self) -> None:
        self.session_id = str(uuid.uuid4())
        self.started = False
        self.history.append(("system", f"Started new session: {self.session_id}"))

    def render(self) -> None:
        clear_screen()
        print("Claude Code – Simple Python TUI\n")
        print(f"Session: {self.session_id}")
        if SDK_URL:
            print(f"SDK URL: {SDK_URL}")
        print("Commands: /reset, /quit\n")
        for role, text in self.history:
            prefix = {"user": "You", "assistant": "Claude"}.get(role, role)
            print(f"{prefix}:")
            print(text.strip(), end="\n\n")

    def run_turn(self, message: str) -> None:
        self.history.append(("user", message))
        args = [
            CLAUDE_BIN,
            "--print",
            "--output-format",
            "json",
        ]
        if self.started:
            args.extend(["--resume", self.session_id])
        else:
            args.extend(["--session-id", self.session_id])
            self.started = True
        if SDK_URL:
            args.extend(["--sdk-url", SDK_URL])

        proc = subprocess.Popen(
            args + [message],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        stdout, stderr = proc.communicate()
        if proc.returncode != 0:
            self.history.append(
                (
                    "error",
                    f"Claude CLI exited with code {proc.returncode}:\n{stderr.strip()}",
                )
            )
            return
        try:
            payload = json.loads(stdout.strip())
            text = "\n".join(
                block.get("text", "") for block in payload.get("content", [])
            ).strip()
            text = text or stdout.strip()
        except json.JSONDecodeError:
            text = f"[parse error]\n{stdout.strip()}"
        self.history.append(("assistant", text))

    def _initial_session_id(self) -> str:
        existing = os.environ.get("SESSION_ID")
        if existing:
            try:
                return str(uuid.UUID(existing))
            except ValueError:
                print(
                    "Warning: SESSION_ID was not a valid UUID; generating a new one.",
                    file=sys.stderr,
                )
        return str(uuid.uuid4())


def main() -> None:
    session = Session()
    session.render()

    try:
        while True:
            try:
                line = input("> ").strip()
            except EOFError:
                break
            if not line:
                session.render()
                continue
            if line == "/quit":
                break
            if line == "/reset":
                session.reset()
                session.render()
                continue
            session.run_turn(line)
            session.render()
    except KeyboardInterrupt:
        pass
    finally:
        print("\nGoodbye!")


if __name__ == "__main__":
    main()
