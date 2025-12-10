// ABOUTME: This module handles spawning the Claude CLI process and parsing its JSON responses.
// ABOUTME: It provides parse_response() for extracting text from Claude's JSON output and invoke_claude() for process execution.

use anyhow::{Context, Result};
use serde::Deserialize;
use tokio::process::Command;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct PermissionDenial {
    tool_name: Option<String>,
    tool_use_id: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ClaudeResponse {
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    subtype: Option<String>,
    #[serde(default)]
    is_error: bool,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    permission_denials: Vec<PermissionDenial>,
}

pub fn parse_response(json: &str) -> Result<String> {
    let response: ClaudeResponse =
        serde_json::from_str(json).context("Failed to parse Claude JSON response")?;

    // If there's a result field, return it
    if let Some(result) = response.result {
        return Ok(result);
    }

    // Handle error_during_execution - this happens when Claude hits permission denials or other runtime errors
    if response.subtype.as_deref() == Some("error_during_execution") {
        let mut error_msg = String::from("Claude encountered an error during execution");

        // Check for permission denials (MCP tools that were blocked)
        if !response.permission_denials.is_empty() {
            error_msg.push_str(":\n\nPermission Denials:");
            for denial in &response.permission_denials {
                if let Some(tool) = &denial.tool_name {
                    error_msg.push_str(&format!("\n- Tool: {}", tool));
                    if let Some(reason) = &denial.reason {
                        error_msg.push_str(&format!(" ({})", reason));
                    }
                }
            }
            error_msg.push_str("\n\nPlease approve the tool permissions and try again.");
        }

        anyhow::bail!("{}", error_msg);
    }

    // If no result but there's an error indicator, provide helpful message
    if response.is_error {
        let error_details = response
            .error
            .or(response.message)
            .unwrap_or_else(|| "No error details provided".to_string());
        anyhow::bail!(
            "Claude reported an error (subtype: {}): {}",
            response.subtype.unwrap_or_else(|| "unknown".to_string()),
            error_details
        );
    }

    // If no result field at all, show what we got (log full JSON for debugging)
    tracing::error!(
        full_json = %json,
        "Claude response missing 'result' field - full JSON logged"
    );
    let error_details = response
        .error
        .or(response.message)
        .unwrap_or_else(|| "No additional details".to_string());
    anyhow::bail!(
        "Claude response missing 'result' field (subtype: {}): {}",
        response.subtype.unwrap_or_else(|| "unknown".to_string()),
        error_details
    );
}

pub async fn invoke_claude(
    binary_path: &str,
    sdk_url: Option<&str>,
    session_args: Vec<&str>,
    prompt: &str,
    working_dir: Option<&str>,
) -> Result<String> {
    // Validate binary path doesn't contain suspicious characters
    if binary_path.contains("..") || binary_path.contains('\0') {
        anyhow::bail!("Invalid claude binary path");
    }

    let mut args = vec!["--print", "--output-format", "json"];
    args.extend(session_args);

    if let Some(url) = sdk_url {
        args.extend(["--sdk-url", url]);
    }

    args.push(prompt);

    tracing::debug!(?args, working_dir, "Spawning Claude CLI");

    let mut command = Command::new(binary_path);
    command.args(&args);

    // Set working directory if specified
    if let Some(dir) = working_dir {
        // Validate directory exists
        if !std::path::Path::new(dir).exists() {
            anyhow::bail!("Working directory does not exist: {}", dir);
        }
        command.current_dir(dir);
        tracing::info!(working_dir = dir, "Running Claude in specified directory");
    }

    let output = command.output().await.with_context(|| {
        if let Some(dir) = working_dir {
            format!("Failed to spawn claude CLI in directory: {}", dir)
        } else {
            "Failed to spawn claude CLI".to_string()
        }
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "Claude CLI failed with exit code {:?}: {}",
            output.status.code(),
            stderr
        );
    }

    let stdout = String::from_utf8(output.stdout).context("Claude output is not valid UTF-8")?;

    tracing::debug!(stdout_preview = %stdout.chars().take(500).collect::<String>(), "Claude raw output");

    // parse_response now handles its own error logging with full JSON
    parse_response(&stdout)
}
