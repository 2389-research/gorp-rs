#!/usr/bin/env node
/**
 * Minimal terminal UI that proxies user input to the Claude CLI.
 * It keeps a single session alive and repaints the chat log after each turn.
 */
const { spawn } = require("node:child_process");
const readline = require("node:readline");
const crypto = require("node:crypto");
const os = require("node:os");

const chalk = require("chalk");

const CLAUDE_BIN = process.env.CLAUDE_BIN || "claude";
const sdkUrl = process.env.SDK_URL;
let sessionId =
  process.env.SESSION_ID ||
  crypto.randomUUID().replace(/-/g, ""); // keep it short for the UI
let hasStarted = false;

const history = [];

const rl = readline.createInterface({
  input: process.stdin,
  output: process.stdout,
  prompt: chalk.green("> "),
});

function banner() {
  return [
    chalk.cyan("Claude Code – Simple TUI Demo"),
    chalk.dim(
      `session: ${sessionId} (set SESSION_ID to reuse, type /reset to start over)`
    ),
    chalk.dim("Type `/quit` to exit."),
    "",
  ].join(os.EOL);
}

function render() {
  console.clear();
  console.log(banner());
  for (const entry of history) {
    const prefix =
      entry.role === "user" ? chalk.green("You ") : chalk.blue("Claude");
    console.log(prefix + chalk.dim(":"));
    console.log(entry.text + os.EOL);
  }
  rl.prompt();
}

async function runTurn(message) {
  history.push({ role: "user", text: message });
  render();

  const args = [
    "--print",
    "--output-format",
    "json",
  ];
  if (!hasStarted) {
    args.push("--session-id", sessionId);
    hasStarted = true;
  } else {
    args.push("--resume", sessionId);
  }
  if (sdkUrl) {
    args.push("--sdk-url", sdkUrl);
  }

  const child = spawn(CLAUDE_BIN, [...args, message], {
    stdio: ["ignore", "pipe", "pipe"],
  });

  let stdoutBuffer = "";
  let stderrBuffer = "";

  child.stdout.on("data", (chunk) => {
    stdoutBuffer += chunk.toString("utf8");
  });

  child.stderr.on("data", (chunk) => {
    stderrBuffer += chunk.toString("utf8");
  });

  child.on("close", (code) => {
    if (code !== 0) {
      history.push({
        role: "error",
        text:
          chalk.red("Claude CLI exited with code ") +
          code +
          (stderrBuffer ? `\n${stderrBuffer}` : ""),
      });
      render();
      return;
    }
    try {
      const parsed = JSON.parse(stdoutBuffer.trim());
      const text =
        parsed?.content?.map((c) => c.text || "").join("\n").trim() ||
        stdoutBuffer.trim();
      history.push({ role: "assistant", text });
    } catch (err) {
      history.push({
        role: "error",
        text:
          chalk.red("Failed to parse CLI output: ") +
          err.message +
          "\n" +
          stdoutBuffer,
      });
    }
    render();
  });

}

function resetSession() {
  sessionId = crypto.randomUUID().replace(/-/g, "");
  hasStarted = false;
  history.push({
    role: "system",
    text: chalk.yellow(`Started new session: ${sessionId}`),
  });
  render();
}

render();
rl.on("line", (line) => {
  const trimmed = line.trim();
  if (!trimmed) {
    render();
    return;
  }
  if (trimmed === "/quit") {
    rl.close();
    return;
  }
  if (trimmed === "/reset") {
    resetSession();
    return;
  }
  runTurn(trimmed);
});

rl.on("SIGINT", () => {
  rl.close();
});

rl.on("close", () => {
  console.log(chalk.dim("\nGoodbye!"));
  process.exit(0);
});
