// ABOUTME: Simple interactive REPL for testing gorp-agent backends.
// ABOUTME: Usage: agent-repl [direct|claude-acp|codex-acp]

use gorp_agent::backends::acp::{AcpBackend, AcpConfig};
use gorp_agent::backends::direct_cli::{DirectCliBackend, DirectCliConfig};
use gorp_agent::{AgentEvent, AgentHandle};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

fn print_colored(color: &str, text: &str) {
    let code = match color {
        "green" => "\x1b[32m",
        "yellow" => "\x1b[33m",
        "blue" => "\x1b[34m",
        "magenta" => "\x1b[35m",
        "cyan" => "\x1b[36m",
        "red" => "\x1b[31m",
        "dim" => "\x1b[2m",
        "bold" => "\x1b[1m",
        _ => "",
    };
    print!("{}{}\x1b[0m", code, text);
}

fn println_colored(color: &str, text: &str) {
    print_colored(color, text);
    println!();
}

fn print_help() {
    println!();
    println_colored("bold", "Commands:");
    println!("  /new      - Start a new session");
    println!("  /session  - Show current session ID");
    println!("  /quit     - Exit the REPL");
    println!("  /help     - Show this help");
    println!();
    println!("Type anything else to send as a prompt.");
    println!();
}

fn create_backend(backend_type: &str) -> Result<AgentHandle, String> {
    let working_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    match backend_type {
        "direct" => {
            let config = DirectCliConfig {
                binary: std::env::var("CLAUDE_BINARY").unwrap_or_else(|_| "claude".to_string()),
                sdk_url: std::env::var("CLAUDE_SDK_URL").ok(),
                working_dir,
            };
            let backend = DirectCliBackend::new(config).map_err(|e| e.to_string())?;
            Ok(backend.into_handle())
        }
        "claude-acp" => {
            let config = AcpConfig {
                binary: "claude-code-acp".to_string(),
                timeout_secs: 300,
                working_dir,
                extra_args: vec![],
            };
            let backend = AcpBackend::new(config).map_err(|e| e.to_string())?;
            Ok(backend.into_handle())
        }
        "codex-acp" => {
            // Use -c flags for full permissions via codex-acp config overrides
            // (equivalent to claude's --dangerously-skip-permissions)
            let config = AcpConfig {
                binary: "codex-acp".to_string(),
                timeout_secs: 300,
                working_dir,
                extra_args: vec![
                    "-c".to_string(),
                    "sandbox_mode=\"danger-full-access\"".to_string(),
                ],
            };
            let backend = AcpBackend::new(config).map_err(|e| e.to_string())?;
            Ok(backend.into_handle())
        }
        _ => Err(format!("Unknown backend: {}", backend_type)),
    }
}

async fn run_prompt(handle: &AgentHandle, session: &str, prompt: &str) -> Option<String> {
    let mut rx = match handle.prompt(session, prompt).await {
        Ok(rx) => rx,
        Err(e) => {
            println_colored("red", &format!("Error: {}", e));
            return None;
        }
    };

    let mut new_session_id = None;

    while let Some(event) = rx.recv().await {
        match event {
            AgentEvent::Text(text) => {
                print!("{}", text);
                io::stdout().flush().ok();
            }
            AgentEvent::ToolStart { name, .. } => {
                println!(); // Ensure newline before tool
                print_colored("cyan", "┌─ ");
                print_colored("bold", &name);
                println!();
                io::stdout().flush().ok();
            }
            AgentEvent::ToolEnd { success, .. } => {
                print_colored("cyan", "└─ ");
                if success {
                    println_colored("green", "done");
                } else {
                    println_colored("red", "failed");
                }
            }
            AgentEvent::ToolProgress { update, .. } => {
                print_colored("cyan", "│  ");
                println_colored("dim", &format!("{}", update));
            }
            AgentEvent::Result { usage, .. } => {
                println!();
                if let Some(u) = usage {
                    println_colored(
                        "dim",
                        &format!(
                            "[tokens: {} in, {} out{}]",
                            u.input_tokens,
                            u.output_tokens,
                            u.cost_usd
                                .map(|c| format!(", ${:.4}", c))
                                .unwrap_or_default()
                        ),
                    );
                }
            }
            AgentEvent::Error { message, code, .. } => {
                println_colored("red", &format!("\n[Error {:?}] {}", code, message));
            }
            AgentEvent::SessionChanged { new_session_id: id } => {
                new_session_id = Some(id);
            }
            AgentEvent::SessionInvalid { reason } => {
                println_colored("yellow", &format!("\n[Session Invalid] {}", reason));
            }
            AgentEvent::Custom { kind, payload } => {
                if kind == "thinking" {
                    // Display codex thinking/status updates nicely
                    if let Some(status) = payload.get("status").and_then(|s| s.as_str()) {
                        println!(); // Ensure newline before
                        println_colored("dim", &format!("... {}", status));
                    }
                } else {
                    println_colored("magenta", &format!("\n[{}] {:?}", kind, payload));
                }
            }
        }
    }

    new_session_id
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    let backend_type = args.get(1).map(|s| s.as_str()).unwrap_or("direct");

    println!();
    println_colored("bold", "╔══════════════════════════════════════╗");
    println_colored("bold", "║       gorp-agent Interactive REPL    ║");
    println_colored("bold", "╚══════════════════════════════════════╝");
    println!();
    println!("Backend: {}", backend_type);

    let handle = create_backend(backend_type)?;

    println!("Creating new session...");
    let initial_session = handle.new_session().await?;
    let mut session = initial_session;
    println_colored("green", &format!("Session: {}", session));
    print_help();

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        print_colored("bold", ">>> ");
        stdout.flush()?;

        let mut input = String::new();
        if stdin.lock().read_line(&mut input)? == 0 {
            break; // EOF
        }

        let input = input.trim();
        if input.is_empty() {
            continue;
        }

        match input {
            "/quit" | "/exit" | "/q" => {
                println_colored("dim", "Goodbye!");
                break;
            }
            "/new" => {
                println!("Creating new session...");
                match handle.new_session().await {
                    Ok(id) => {
                        session = id;
                        println_colored("green", &format!("New session: {}", session));
                    }
                    Err(e) => {
                        println_colored("red", &format!("Error: {}", e));
                    }
                }
            }
            "/session" => {
                println!("Current session: {}", session);
            }
            "/help" | "/?" => {
                print_help();
            }
            _ => {
                println!();
                if let Some(new_id) = run_prompt(&handle, &session, input).await {
                    session = new_id;
                }
                println!();
            }
        }
    }

    Ok(())
}
