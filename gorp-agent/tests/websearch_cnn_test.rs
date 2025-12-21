// ABOUTME: Quick integration test for websearch via CNN homepage.
// ABOUTME: Tests DirectCli and ACP backends with real websearch.

use gorp_agent::backends::direct_cli::{DirectCliBackend, DirectCliConfig};
use gorp_agent::AgentEvent;
use std::path::PathBuf;
use std::time::Duration;
use tokio::time::timeout;

fn real_claude_config() -> DirectCliConfig {
    DirectCliConfig {
        binary: std::env::var("CLAUDE_BINARY").unwrap_or_else(|_| "claude".to_string()),
        sdk_url: std::env::var("CLAUDE_SDK_URL").ok(),
        working_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    }
}

async fn collect_events(
    handle: &gorp_agent::AgentHandle,
    session: &str,
    prompt: &str,
    timeout_secs: u64,
) -> Result<(Vec<AgentEvent>, Option<String>), String> {
    let mut rx = handle
        .prompt(session, prompt)
        .await
        .map_err(|e| format!("Prompt failed: {}", e))?;

    let mut events = vec![];
    let deadline = Duration::from_secs(timeout_secs);

    loop {
        match timeout(deadline, rx.recv()).await {
            Ok(Some(event)) => events.push(event),
            Ok(None) => break,
            Err(_) => return Err(format!("Timeout after {}s", timeout_secs)),
        }
    }

    let real_session_id = events.iter().find_map(|e| match e {
        AgentEvent::SessionChanged { new_session_id } => Some(new_session_id.clone()),
        _ => None,
    });

    Ok((events, real_session_id))
}

#[tokio::test]
#[ignore = "Requires real Claude CLI with web access"]
async fn test_websearch_cnn_direct_cli() {
    println!("\n=== Testing WebSearch on CNN via DirectCli Backend ===\n");

    let config = real_claude_config();
    let backend = DirectCliBackend::new(config).expect("Failed to create backend");
    let handle = backend.into_handle();

    let session = handle.new_session().await.expect("Failed to create session");

    let prompt = "Use web search to check CNN.com and tell me the top 3 headlines you find. Be brief.";

    println!("Prompt: {}", prompt);
    println!("Waiting for response...\n");

    let (events, _) = collect_events(&handle, &session, prompt, 180)
        .await
        .expect("Failed to get response");

    // Print all events for debugging
    println!("--- Events ---");
    for event in &events {
        match event {
            AgentEvent::Text(t) => print!("{}", t),
            AgentEvent::ToolStart { name, .. } => println!("\n[Tool: {}]", name),
            AgentEvent::ToolEnd { name, success, .. } => {
                println!("[Tool {} completed: success={}]", name, success)
            }
            AgentEvent::Result { text, usage, .. } => {
                println!("\n--- Result ---\n{}", text);
                if let Some(u) = usage {
                    println!(
                        "\n[Usage: {} input, {} output tokens]",
                        u.input_tokens, u.output_tokens
                    );
                }
            }
            AgentEvent::Error { message, .. } => println!("\n[ERROR: {}]", message),
            _ => {}
        }
    }

    // Check for websearch tool usage
    let search_tools: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ToolStart { name, .. }
                if name.to_lowercase().contains("search")
                    || name.to_lowercase().contains("web")
                    || name.to_lowercase().contains("fetch") =>
            {
                Some(name.clone())
            }
            _ => None,
        })
        .collect();

    println!("\n--- Web Tools Used ---");
    if search_tools.is_empty() {
        println!("No web search/fetch tools detected");
    } else {
        for tool in &search_tools {
            println!("  - {}", tool);
        }
    }

    // Get result text
    let result_text: String = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::Result { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();

    assert!(!result_text.is_empty(), "Expected a result");
    println!("\n=== DirectCli Test Complete ===\n");
}

#[cfg(feature = "acp")]
async fn run_acp_websearch_test(binary_name: &str, display_name: &str) {
    use gorp_agent::backends::acp::{AcpBackend, AcpConfig};

    println!("\n=== Testing WebSearch on CNN via {} ===\n", display_name);

    // Check if ACP binary is available
    if std::process::Command::new(binary_name)
        .arg("--help")
        .output()
        .is_err()
    {
        println!("Skipping: {} not available", binary_name);
        return;
    }

    // Use -c flags for codex to match claude's permissions
    let extra_args = if binary_name == "codex-acp" {
        vec![
            "-c".to_string(),
            "sandbox_mode=\"danger-full-access\"".to_string(),
        ]
    } else {
        vec![]
    };

    let config = AcpConfig {
        binary: binary_name.to_string(),
        timeout_secs: 300,
        working_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        extra_args,
    };

    let backend = AcpBackend::new(config).expect("Failed to create ACP backend");
    let handle = backend.into_handle();

    let session = handle.new_session().await.expect("Failed to create session");

    let prompt = "Use web search to check CNN.com and tell me the top 3 headlines you find. Be brief.";

    println!("Prompt: {}", prompt);
    println!("Waiting for response...\n");

    let (events, _) = collect_events(&handle, &session, prompt, 300)
        .await
        .expect("Failed to get response");

    // Print all events
    println!("--- Events ({}) ---", display_name);
    for event in &events {
        match event {
            AgentEvent::Text(t) => print!("{}", t),
            AgentEvent::ToolStart { name, .. } => println!("\n[Tool: {}]", name),
            AgentEvent::ToolEnd { name, success, .. } => {
                println!("[Tool {} completed: success={}]", name, success)
            }
            AgentEvent::Result { text, .. } => {
                println!("\n--- Result ---\n{}", text);
            }
            AgentEvent::Error { message, .. } => println!("\n[ERROR: {}]", message),
            _ => {}
        }
    }

    // Check for web tools
    let web_tools: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ToolStart { name, .. }
                if name.to_lowercase().contains("search")
                    || name.to_lowercase().contains("web")
                    || name.to_lowercase().contains("fetch") =>
            {
                Some(name.clone())
            }
            _ => None,
        })
        .collect();

    println!("\n--- Web Tools Used ({}) ---", display_name);
    if web_tools.is_empty() {
        println!("No web tools detected");
    } else {
        for tool in &web_tools {
            println!("  - {}", tool);
        }
    }

    println!("\n=== {} Test Complete ===\n", display_name);
}

#[cfg(feature = "acp")]
#[tokio::test]
#[ignore = "Requires claude-code-acp with web access"]
async fn test_websearch_cnn_claude_code_acp() {
    run_acp_websearch_test("claude-code-acp", "Claude Code ACP").await;
}

#[cfg(feature = "acp")]
#[tokio::test]
#[ignore = "Requires codex-acp with web access"]
async fn test_websearch_cnn_codex_acp() {
    run_acp_websearch_test("codex-acp", "Codex ACP").await;
}
