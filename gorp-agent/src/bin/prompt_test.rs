// ABOUTME: Simple CLI for testing session resume and timing.
// ABOUTME: Usage: prompt-test [--config <file>] <backend> <session-id> <prompt>

use gorp_agent::backends::acp::{AcpBackend, AcpConfig};
use gorp_agent::backends::direct_cli::{DirectCliBackend, DirectCliConfig};
use gorp_agent::backends::direct_codex::{DirectCodexBackend, DirectCodexConfig};
use gorp_agent::{AgentEvent, AgentRegistry, Config};
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Instant;

fn create_backend(backend_type: &str) -> Result<gorp_agent::AgentHandle, String> {
    let working_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    match backend_type {
        "direct" | "direct-claude" => {
            let config = DirectCliConfig {
                binary: std::env::var("CLAUDE_BINARY").unwrap_or_else(|_| "claude".to_string()),
                sdk_url: std::env::var("CLAUDE_SDK_URL").ok(),
                working_dir,
            };
            let backend = DirectCliBackend::new(config).map_err(|e| e.to_string())?;
            Ok(backend.into_handle())
        }
        "direct-codex" => {
            let config = DirectCodexConfig {
                binary: std::env::var("CODEX_BINARY").unwrap_or_else(|_| "codex".to_string()),
                working_dir,
                sandbox_mode: "danger-full-access".to_string(),
            };
            let backend = DirectCodexBackend::new(config).map_err(|e| e.to_string())?;
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

fn create_backend_from_config(config_path: &str) -> Result<gorp_agent::AgentHandle, String> {
    let config = Config::from_file(std::path::Path::new(config_path))
        .map_err(|e| format!("Failed to load config: {}", e))?;

    let registry = AgentRegistry::default();
    registry
        .create_from_config(&config.backend)
        .map_err(|e| format!("Failed to create backend: {}", e))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    // Parse --config flag
    let (config_path, remaining_args): (Option<&str>, Vec<&str>) = {
        let mut config_path = None;
        let mut remaining = Vec::new();
        let mut skip_next = false;

        for (i, arg) in args.iter().enumerate().skip(1) {
            if skip_next {
                skip_next = false;
                continue;
            }
            if arg == "--config" || arg == "-c" {
                if let Some(next) = args.get(i + 1) {
                    config_path = Some(next.as_str());
                    skip_next = true;
                }
            } else {
                remaining.push(arg.as_str());
            }
        }
        (config_path, remaining)
    };

    // With --config, we only need session and prompt
    let min_args = if config_path.is_some() { 2 } else { 3 };

    if remaining_args.len() < min_args {
        eprintln!(
            "Usage: {} [--config <file>] <backend> <session-id> <prompt>",
            args[0]
        );
        eprintln!("       {} --config <file> <session-id> <prompt>", args[0]);
        eprintln!();
        eprintln!("Backends (when not using --config):");
        eprintln!("  direct-claude  - Claude CLI directly (--resume for sessions)");
        eprintln!("  direct-codex   - Codex CLI directly (exec resume for sessions)");
        eprintln!("  claude-acp     - Claude via ACP protocol (session/resume)");
        eprintln!("  codex-acp      - Codex via ACP protocol (no session resume)");
        eprintln!();
        eprintln!("Config file format (TOML):");
        eprintln!("  [backend]");
        eprintln!("  type = \"direct\"");
        eprintln!("  binary = \"claude\"");
        eprintln!("  working_dir = \".\"");
        eprintln!();
        eprintln!("Session:  'new' to create new session, or existing session ID");
        eprintln!();
        eprintln!("Examples:");
        eprintln!("  {} direct-codex new \"what is 1+1\"", args[0]);
        eprintln!("  {} --config agent.toml new \"hello\"", args[0]);
        eprintln!("  {} claude-acp abc-123 \"what is my name?\"", args[0]);
        std::process::exit(1);
    }

    let (session_arg, prompt) = if config_path.is_some() {
        (remaining_args[0], remaining_args[1])
    } else {
        (remaining_args[1], remaining_args[2])
    };

    let start = Instant::now();

    let handle = if let Some(cfg_path) = config_path {
        eprintln!("[config: {}]", cfg_path);
        create_backend_from_config(cfg_path)?
    } else {
        let backend_type = remaining_args[0];
        create_backend(backend_type)?
    };

    let init_time = start.elapsed();
    eprintln!("[init: {:?}]", init_time);

    // Get or create session
    let session_start = Instant::now();
    let session = if session_arg == "new" {
        let id = handle.new_session().await?;
        eprintln!("[new session: {}]", id);
        id
    } else {
        handle.load_session(session_arg).await?;
        eprintln!("[loaded session: {}]", session_arg);
        session_arg.to_string()
    };
    let session_time = session_start.elapsed();
    eprintln!("[session: {:?}]", session_time);

    // Send prompt
    let prompt_start = Instant::now();
    let mut rx = handle.prompt(&session, prompt).await?;

    // Collect and output events
    while let Some(event) = rx.recv().await {
        match event {
            AgentEvent::Text(text) => {
                print!("{}", text);
                io::stdout().flush().ok();
            }
            AgentEvent::ToolStart { name, .. } => {
                eprintln!("[tool: {}]", name);
            }
            AgentEvent::ToolEnd { name, success, .. } => {
                eprintln!(
                    "[tool {} done: {}]",
                    name,
                    if success { "ok" } else { "fail" }
                );
            }
            AgentEvent::Result { usage, .. } => {
                println!();
                if let Some(u) = usage {
                    eprintln!(
                        "[tokens: {} in, {} out{}]",
                        u.input_tokens,
                        u.output_tokens,
                        u.cost_usd
                            .map(|c| format!(", ${:.4}", c))
                            .unwrap_or_default()
                    );
                }
            }
            AgentEvent::Error { message, .. } => {
                eprintln!("[error: {}]", message);
            }
            AgentEvent::SessionChanged { new_session_id } => {
                eprintln!("[session changed: {}]", new_session_id);
            }
            AgentEvent::SessionInvalid { reason } => {
                eprintln!("[session invalid: {}]", reason);
            }
            AgentEvent::Custom { kind, payload } => {
                if kind == "thinking" {
                    if let Some(status) = payload.get("status").and_then(|s| s.as_str()) {
                        eprintln!("[thinking: {}]", status);
                    }
                }
            }
            _ => {}
        }
    }

    let prompt_time = prompt_start.elapsed();
    let total_time = start.elapsed();

    eprintln!("[prompt: {:?}]", prompt_time);
    eprintln!("[total: {:?}]", total_time);

    Ok(())
}
