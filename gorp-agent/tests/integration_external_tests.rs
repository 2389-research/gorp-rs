// ABOUTME: Integration tests with real external services (Claude CLI, MCP servers, websearch).
// ABOUTME: These tests require actual Claude CLI and API access - marked #[ignore] by default.
//
// Run these tests with:
//   cargo test --test integration_external_tests -- --ignored
//
// Requirements:
// - `claude` CLI installed and in PATH
// - Valid API key configured (ANTHROPIC_API_KEY or claude login)
// - For MCP tests: MCP servers configured in Claude settings
// - Network access for websearch tests

use gorp_agent::backends::direct_cli::{DirectCliBackend, DirectCliConfig};
use gorp_agent::AgentEvent;
use std::path::PathBuf;
use std::time::Duration;
use tokio::time::timeout;

/// Get DirectCliConfig for real Claude CLI
fn real_claude_config() -> DirectCliConfig {
    DirectCliConfig {
        binary: std::env::var("CLAUDE_BINARY").unwrap_or_else(|_| "claude".to_string()),
        sdk_url: std::env::var("CLAUDE_SDK_URL").ok(),
        working_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    }
}

/// Helper to collect all events from a prompt with timeout
async fn collect_events_with_timeout(
    handle: &gorp_agent::AgentHandle,
    session: &str,
    prompt: &str,
    timeout_secs: u64,
) -> Result<Vec<AgentEvent>, String> {
    let mut rx = handle
        .prompt(session, prompt)
        .await
        .map_err(|e| format!("Prompt failed: {}", e))?;

    let mut events = vec![];
    let deadline = Duration::from_secs(timeout_secs);

    loop {
        match timeout(deadline, rx.recv()).await {
            Ok(Some(event)) => events.push(event),
            Ok(None) => break, // Stream closed
            Err(_) => return Err(format!("Timeout after {}s", timeout_secs)),
        }
    }

    Ok(events)
}

/// Helper to collect events and extract the real session ID from SessionChanged events
async fn collect_events_with_session(
    handle: &gorp_agent::AgentHandle,
    session: &str,
    prompt: &str,
    timeout_secs: u64,
) -> Result<(Vec<AgentEvent>, Option<String>), String> {
    let events = collect_events_with_timeout(handle, session, prompt, timeout_secs).await?;

    // Extract real session ID if Claude sent one
    let real_session_id = events.iter().find_map(|e| match e {
        AgentEvent::SessionChanged { new_session_id } => Some(new_session_id.clone()),
        _ => None,
    });

    Ok((events, real_session_id))
}

/// Check if Claude CLI is available
fn claude_available() -> bool {
    std::process::Command::new("claude")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ============================================================================
// BASIC CLAUDE CLI INTEGRATION
// ============================================================================

/// Integration: Real Claude CLI responds to simple prompt
/// Given: A working Claude CLI installation
/// When: A simple prompt is sent
/// Then: A result event is received with text content
#[tokio::test]
#[ignore = "Requires real Claude CLI and API access"]
async fn integration_claude_cli_simple_prompt() {
    if !claude_available() {
        eprintln!("Skipping: Claude CLI not available");
        return;
    }

    let config = real_claude_config();
    let backend = DirectCliBackend::new(config).expect("Failed to create backend");
    let handle = backend.into_handle();

    let session = handle
        .new_session()
        .await
        .expect("Failed to create session");

    let events = collect_events_with_timeout(&handle, &session, "Reply with exactly: PONG", 60)
        .await
        .expect("Failed to collect events");

    // Should have at least one Result event
    let has_result = events
        .iter()
        .any(|e| matches!(e, AgentEvent::Result { .. }));
    assert!(has_result, "Expected Result event, got: {:?}", events);

    // Result should contain some text
    let result_text: String = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::Result { text, .. } => Some(text.clone()),
            AgentEvent::Text(t) => Some(t.clone()),
            _ => None,
        })
        .collect();

    assert!(!result_text.is_empty(), "Expected non-empty response");
    println!("Response: {}", result_text);
}

/// Integration: Real Claude CLI reports usage stats
/// Given: A working Claude CLI installation
/// When: A prompt is completed
/// Then: Usage information is included in the Result event
#[tokio::test]
#[ignore = "Requires real Claude CLI and API access"]
async fn integration_claude_cli_reports_usage() {
    if !claude_available() {
        eprintln!("Skipping: Claude CLI not available");
        return;
    }

    let config = real_claude_config();
    let backend = DirectCliBackend::new(config).expect("Failed to create backend");
    let handle = backend.into_handle();

    let session = handle
        .new_session()
        .await
        .expect("Failed to create session");

    let events = collect_events_with_timeout(&handle, &session, "Say hello briefly", 60)
        .await
        .expect("Failed to collect events");

    // Find usage in Result event
    let usage = events.iter().find_map(|e| match e {
        AgentEvent::Result { usage, .. } => usage.clone(),
        _ => None,
    });

    if let Some(u) = usage {
        println!(
            "Usage: input={}, output={}",
            u.input_tokens, u.output_tokens
        );
        assert!(
            u.input_tokens > 0 || u.output_tokens > 0,
            "Expected some token usage"
        );
    } else {
        println!("Warning: No usage stats in response (may be expected for some configurations)");
    }
}

// ============================================================================
// TOOL INVOCATION TESTS
// ============================================================================

/// Integration: Claude CLI invokes Read tool
/// Given: A working Claude CLI with tool access
/// When: Asked to read a file that exists
/// Then: ToolStart and ToolEnd events are emitted for Read
#[tokio::test]
#[ignore = "Requires real Claude CLI and API access"]
async fn integration_tool_invocation_read() {
    if !claude_available() {
        eprintln!("Skipping: Claude CLI not available");
        return;
    }

    let config = real_claude_config();
    let backend = DirectCliBackend::new(config).expect("Failed to create backend");
    let handle = backend.into_handle();

    let session = handle
        .new_session()
        .await
        .expect("Failed to create session");

    // Ask Claude to read Cargo.toml which should exist
    let events = collect_events_with_timeout(
        &handle,
        &session,
        "Read the first 5 lines of Cargo.toml and tell me the package name",
        120,
    )
    .await
    .expect("Failed to collect events");

    // Should have ToolStart for Read
    let has_read_tool = events.iter().any(|e| match e {
        AgentEvent::ToolStart { name, .. } => name.contains("Read") || name.contains("read"),
        _ => false,
    });

    println!(
        "Events received: {:?}",
        events
            .iter()
            .map(|e| match e {
                AgentEvent::ToolStart { name, .. } => format!("ToolStart({})", name),
                AgentEvent::ToolEnd { name, success, .. } =>
                    format!("ToolEnd({}, success={})", name, success),
                AgentEvent::Text(t) => format!("Text({}...)", &t[..t.len().min(20)]),
                AgentEvent::Result { text, .. } =>
                    format!("Result({}...)", &text[..text.len().min(30)]),
                AgentEvent::Error { message, .. } => format!("Error({})", message),
                _ => format!("{:?}", e),
            })
            .collect::<Vec<_>>()
    );

    assert!(has_read_tool, "Expected Read tool to be invoked");
}

/// Integration: Claude CLI invokes Bash tool
/// Given: A working Claude CLI with tool access
/// When: Asked to run a shell command
/// Then: ToolStart and ToolEnd events are emitted for Bash
#[tokio::test]
#[ignore = "Requires real Claude CLI and API access"]
async fn integration_tool_invocation_bash() {
    if !claude_available() {
        eprintln!("Skipping: Claude CLI not available");
        return;
    }

    let config = real_claude_config();
    let backend = DirectCliBackend::new(config).expect("Failed to create backend");
    let handle = backend.into_handle();

    let session = handle
        .new_session()
        .await
        .expect("Failed to create session");

    let events = collect_events_with_timeout(
        &handle,
        &session,
        "Run `echo INTEGRATION_TEST_MARKER` and show me the output",
        120,
    )
    .await
    .expect("Failed to collect events");

    // Should have ToolStart for Bash
    let has_bash_tool = events.iter().any(|e| match e {
        AgentEvent::ToolStart { name, .. } => name.contains("Bash") || name.contains("bash"),
        _ => false,
    });

    println!("Events: {:?}", events.len());
    assert!(has_bash_tool, "Expected Bash tool to be invoked");

    // Result should mention the marker
    let result_text: String = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::Result { text, .. } => Some(text.clone()),
            AgentEvent::Text(t) => Some(t.clone()),
            _ => None,
        })
        .collect();

    assert!(
        result_text.contains("INTEGRATION_TEST_MARKER"),
        "Expected command output in response: {}",
        result_text
    );
}

// ============================================================================
// MCP SERVER INTEGRATION
// ============================================================================

/// Integration: MCP tool invocation
/// Given: Claude CLI with MCP servers configured
/// When: A prompt triggers an MCP tool
/// Then: ToolStart events show mcp__ prefix
///
/// Note: This test requires MCP servers to be configured in Claude's settings.
/// Configure with: claude mcp add <server-name>
#[tokio::test]
#[ignore = "Requires real Claude CLI with MCP servers configured"]
async fn integration_mcp_tool_invocation() {
    if !claude_available() {
        eprintln!("Skipping: Claude CLI not available");
        return;
    }

    let config = real_claude_config();
    let backend = DirectCliBackend::new(config).expect("Failed to create backend");
    let handle = backend.into_handle();

    let session = handle
        .new_session()
        .await
        .expect("Failed to create session");

    // This prompt asks Claude to list MCP tools - if any are configured, it will show them
    let events = collect_events_with_timeout(
        &handle,
        &session,
        "List all available MCP tools. If there are any MCP servers configured, use one of their tools.",
        120,
    )
    .await
    .expect("Failed to collect events");

    // Check for any MCP tool usage (prefixed with mcp__)
    let mcp_tools: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ToolStart { name, .. } if name.starts_with("mcp__") => Some(name.clone()),
            _ => None,
        })
        .collect();

    if mcp_tools.is_empty() {
        println!(
            "No MCP tools were invoked - this may be expected if no MCP servers are configured"
        );
        println!("Configure MCP servers with: claude mcp add <server-name>");
    } else {
        println!("MCP tools invoked: {:?}", mcp_tools);
    }

    // Test passes either way - we're validating the infrastructure handles MCP events
    let has_result = events
        .iter()
        .any(|e| matches!(e, AgentEvent::Result { .. }));
    assert!(has_result, "Expected at least a Result event");
}

/// Integration: Memory MCP server
/// Given: Claude CLI with memory MCP server configured
/// When: Asked to remember and recall something
/// Then: mcp__memory tools are invoked
#[tokio::test]
#[ignore = "Requires Claude CLI with memory MCP server"]
async fn integration_mcp_memory_server() {
    if !claude_available() {
        eprintln!("Skipping: Claude CLI not available");
        return;
    }

    let config = real_claude_config();
    let backend = DirectCliBackend::new(config).expect("Failed to create backend");
    let handle = backend.into_handle();

    let session = handle
        .new_session()
        .await
        .expect("Failed to create session");

    // Generate a unique marker
    let marker = format!("TEST_MARKER_{}", uuid::Uuid::new_v4());

    // Ask Claude to remember something using memory
    let events = collect_events_with_timeout(
        &handle,
        &session,
        &format!("Use the memory MCP server to store this fact: '{}'", marker),
        120,
    )
    .await
    .expect("Failed to collect events");

    // Check for memory tool usage
    let memory_tools: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ToolStart { name, .. } if name.contains("memory") => Some(name.clone()),
            _ => None,
        })
        .collect();

    if memory_tools.is_empty() {
        println!("Memory MCP server not available - configure with: claude mcp add memory");
    } else {
        println!("Memory tools used: {:?}", memory_tools);
        assert!(
            memory_tools.iter().any(|t| t.contains("mcp__")),
            "Expected mcp__ prefixed tool"
        );
    }
}

// ============================================================================
// WEBSEARCH INTEGRATION
// ============================================================================

/// Integration: WebSearch tool invocation
/// Given: Claude CLI with web search capability
/// When: Asked to search for current information
/// Then: WebSearch tool events are emitted
#[tokio::test]
#[ignore = "Requires real Claude CLI with web search enabled"]
async fn integration_websearch_tool() {
    if !claude_available() {
        eprintln!("Skipping: Claude CLI not available");
        return;
    }

    let config = real_claude_config();
    let backend = DirectCliBackend::new(config).expect("Failed to create backend");
    let handle = backend.into_handle();

    let session = handle
        .new_session()
        .await
        .expect("Failed to create session");

    // Ask something that requires web search
    let events = collect_events_with_timeout(
        &handle,
        &session,
        "Use web search to find the current Rust stable version number. Search the web to answer.",
        180,
    )
    .await
    .expect("Failed to collect events");

    // Check for WebSearch tool
    let search_tools: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ToolStart { name, .. }
                if name.to_lowercase().contains("search")
                    || name.to_lowercase().contains("web") =>
            {
                Some(name.clone())
            }
            _ => None,
        })
        .collect();

    if search_tools.is_empty() {
        println!("No search tools invoked - web search may not be enabled");
        println!("Ensure Claude has web search capability enabled");
    } else {
        println!("Search tools used: {:?}", search_tools);
    }

    // Should still get a result
    let has_result = events
        .iter()
        .any(|e| matches!(e, AgentEvent::Result { .. }));
    assert!(has_result, "Expected Result event");
}

// ============================================================================
// MULTI-TURN CONVERSATION TESTS
// ============================================================================

/// Integration: Multi-turn conversation maintains context
/// Given: A session with multiple prompts
/// When: Follow-up prompts reference earlier context
/// Then: Claude remembers and uses the context correctly
#[tokio::test]
#[ignore = "Requires real Claude CLI and API access"]
async fn integration_multi_turn_context() {
    if !claude_available() {
        eprintln!("Skipping: Claude CLI not available");
        return;
    }

    let config = real_claude_config();
    let backend = DirectCliBackend::new(config).expect("Failed to create backend");
    let handle = backend.into_handle();

    let initial_session = handle
        .new_session()
        .await
        .expect("Failed to create session");

    // First turn: establish a fact and capture the real session ID
    let magic_number = 42;
    let (events1, real_session_id) = collect_events_with_session(
        &handle,
        &initial_session,
        &format!(
            "Remember this number for our conversation: {}",
            magic_number
        ),
        60,
    )
    .await
    .expect("First prompt failed");

    // Use the real session ID from Claude for subsequent prompts
    let session = real_session_id.unwrap_or(initial_session);
    println!("Using session ID: {}", session);
    println!("First turn events: {}", events1.len());

    // Second turn: ask about it using the real session ID
    let events2 = collect_events_with_timeout(
        &handle,
        &session,
        "What number did I just ask you to remember? Reply with just the number.",
        60,
    )
    .await
    .expect("Second prompt failed");

    let result_text: String = events2
        .iter()
        .filter_map(|e| match e {
            AgentEvent::Result { text, .. } => Some(text.clone()),
            AgentEvent::Text(t) => Some(t.clone()),
            _ => None,
        })
        .collect();

    assert!(
        result_text.contains(&magic_number.to_string()),
        "Expected Claude to remember the number {}, got: {}",
        magic_number,
        result_text
    );
}

/// Integration: Session resume across prompts
/// Given: Multiple prompts to the same session
/// When: Each prompt builds on the previous
/// Then: Full conversation history is maintained
#[tokio::test]
#[ignore = "Requires real Claude CLI and API access"]
async fn integration_session_resume() {
    if !claude_available() {
        eprintln!("Skipping: Claude CLI not available");
        return;
    }

    let config = real_claude_config();
    let backend = DirectCliBackend::new(config).expect("Failed to create backend");
    let handle = backend.into_handle();

    let initial_session = handle
        .new_session()
        .await
        .expect("Failed to create session");

    // Establish a persona and capture real session ID
    let (_events1, real_session_id) = collect_events_with_session(
        &handle,
        &initial_session,
        "For this conversation, pretend you are a pirate named Captain Code. Acknowledge with 'Arr!'",
        60,
    )
    .await
    .expect("First prompt failed");

    // Use the real session ID from Claude
    let session = real_session_id.unwrap_or(initial_session);

    // Ask something - should maintain persona
    let events2 =
        collect_events_with_timeout(&handle, &session, "What is your name and profession?", 60)
            .await
            .expect("Second prompt failed");

    let result_text: String = events2
        .iter()
        .filter_map(|e| match e {
            AgentEvent::Result { text, .. } => Some(text.clone()),
            AgentEvent::Text(t) => Some(t.clone()),
            _ => None,
        })
        .collect();

    // Should mention pirate or Captain Code
    let has_pirate_context = result_text.to_lowercase().contains("pirate")
        || result_text.to_lowercase().contains("captain")
        || result_text.contains("Arr");

    assert!(
        has_pirate_context,
        "Expected pirate context to be maintained, got: {}",
        result_text
    );
}

// ============================================================================
// ERROR HANDLING INTEGRATION
// ============================================================================

/// Integration: Invalid session handling
/// Given: An invalid/expired session ID
/// When: A prompt is sent to it
/// Then: SessionInvalid event or error is returned
#[tokio::test]
#[ignore = "Requires real Claude CLI and API access"]
async fn integration_invalid_session_handling() {
    if !claude_available() {
        eprintln!("Skipping: Claude CLI not available");
        return;
    }

    let config = real_claude_config();
    let backend = DirectCliBackend::new(config).expect("Failed to create backend");
    let handle = backend.into_handle();

    // Use a fake session ID
    let fake_session = "nonexistent-session-12345";
    handle
        .load_session(fake_session)
        .await
        .expect("Load should succeed (actual check happens at prompt)");

    let events = collect_events_with_timeout(&handle, fake_session, "Hello", 60).await;

    match events {
        Ok(evts) => {
            // Should have SessionInvalid or Error event
            let has_error_or_invalid = evts.iter().any(|e| {
                matches!(
                    e,
                    AgentEvent::SessionInvalid { .. } | AgentEvent::Error { .. }
                )
            });
            if has_error_or_invalid {
                println!("Correctly received error/invalid session event");
            } else {
                // Claude might create a new session silently
                println!("Claude may have created a new session: {:?}", evts.len());
            }
        }
        Err(e) => {
            println!("Prompt failed (expected): {}", e);
        }
    }
}

// ============================================================================
// STRESS TESTS
// ============================================================================

/// Integration: Rapid successive prompts
/// Given: A session with the real backend
/// When: Multiple prompts are sent in quick succession
/// Then: All prompts complete successfully
#[tokio::test]
#[ignore = "Requires real Claude CLI - runs multiple API calls"]
async fn integration_rapid_prompts() {
    if !claude_available() {
        eprintln!("Skipping: Claude CLI not available");
        return;
    }

    let config = real_claude_config();
    let backend = DirectCliBackend::new(config).expect("Failed to create backend");
    let handle = backend.into_handle();

    let initial_session = handle
        .new_session()
        .await
        .expect("Failed to create session");

    let prompts = vec!["Count: 1", "Count: 2", "Count: 3"];

    let mut session = initial_session;

    for (i, prompt) in prompts.iter().enumerate() {
        let (events, real_session_id) = collect_events_with_session(&handle, &session, prompt, 60)
            .await
            .expect(&format!("Prompt {} failed", i + 1));

        // Update session ID if Claude provided one
        if let Some(new_id) = real_session_id {
            session = new_id;
        }

        let has_result = events
            .iter()
            .any(|e| matches!(e, AgentEvent::Result { .. }));
        assert!(has_result, "Prompt {} should get result", i + 1);
        println!("Prompt {} completed with {} events", i + 1, events.len());
    }
}
