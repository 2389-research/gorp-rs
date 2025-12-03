// ABOUTME: This module handles spawning the Claude CLI process and parsing its JSON responses.
// ABOUTME: It provides parse_response() for extracting text from Claude's JSON output and invoke_claude() for process execution.

use anyhow::{Context, Result};
use serde::Deserialize;
use tokio::process::Command;

#[derive(Debug, Deserialize)]
struct ClaudeResponse {
    content: Vec<ContentBlock>,
}

#[derive(Debug, Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    #[allow(dead_code)]
    block_type: String,
    text: Option<String>,
}

pub fn parse_response(json: &str) -> Result<String> {
    let response: ClaudeResponse = serde_json::from_str(json)
        .context("Failed to parse Claude JSON response")?;

    let text = response
        .content
        .iter()
        .filter_map(|block| block.text.as_deref())
        .collect::<Vec<_>>()
        .join("");

    Ok(text)
}

pub async fn invoke_claude(
    binary_path: &str,
    sdk_url: Option<&str>,
    session_args: Vec<&str>,
    prompt: &str,
) -> Result<String> {
    let mut args = vec!["--print", "--output-format", "json"];
    args.extend(session_args);

    if let Some(url) = sdk_url {
        args.extend(["--sdk-url", url]);
    }

    args.push(prompt);

    tracing::debug!(?args, "Spawning Claude CLI");

    let output = Command::new(binary_path)
        .args(&args)
        .output()
        .await
        .context("Failed to spawn claude CLI")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Claude CLI failed with exit code {:?}: {}", output.status.code(), stderr);
    }

    let stdout = String::from_utf8(output.stdout)
        .context("Claude output is not valid UTF-8")?;

    parse_response(&stdout)
}
