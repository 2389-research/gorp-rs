// ABOUTME: Shared utility functions for text processing and Matrix message formatting
// ABOUTME: Includes markdown-to-HTML conversion, long message chunking, and JSONL logging

use pulldown_cmark::{html, Parser};
use serde::Serialize;
use tokio::fs::{create_dir_all, OpenOptions};
use tokio::io::AsyncWriteExt;

/// Convert markdown to HTML for Matrix message formatting
pub fn markdown_to_html(markdown: &str) -> String {
    let parser = Parser::new(markdown);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);
    html_output
}

/// Split long text into chunks, trying to break at paragraph boundaries
pub fn chunk_message(text: &str, max_chars: usize) -> Vec<String> {
    if text.len() <= max_chars {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();

    for line in text.lines() {
        // If adding this line would exceed limit, save current chunk
        if !current.is_empty() && current.len() + line.len() + 1 > max_chars {
            chunks.push(current.trim().to_string());
            current = String::new();
        }

        // If a single line is too long, split it
        if line.len() > max_chars {
            if !current.is_empty() {
                chunks.push(current.trim().to_string());
                current = String::new();
            }
            // Split long line at word boundaries
            let mut line_part = String::new();
            for word in line.split_whitespace() {
                if line_part.len() + word.len() + 1 > max_chars {
                    if !line_part.is_empty() {
                        chunks.push(line_part.trim().to_string());
                    }
                    line_part = word.to_string();
                } else {
                    if !line_part.is_empty() {
                        line_part.push(' ');
                    }
                    line_part.push_str(word);
                }
            }
            if !line_part.is_empty() {
                current = line_part;
            }
        } else {
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
        }
    }

    if !current.is_empty() {
        chunks.push(current.trim().to_string());
    }

    chunks
}

/// Maximum chunk size for Matrix messages (chars)
pub const MAX_CHUNK_SIZE: usize = 8000;

/// Matrix message log entry for JSONL logging
#[derive(Serialize)]
pub struct MatrixMessageLog {
    pub timestamp: String,
    pub room_id: String,
    pub message_type: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunk_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_chunks: Option<usize>,
}

/// Log a Matrix message to .matrix/matrix-messages.jsonl
pub async fn log_matrix_message(
    working_dir: &str,
    room_id: &str,
    message_type: &str,
    content: &str,
    html: Option<&str>,
    chunk_index: Option<usize>,
    total_chunks: Option<usize>,
) {
    let matrix_dir = format!("{}/.matrix", working_dir);
    if let Err(e) = create_dir_all(&matrix_dir).await {
        tracing::warn!(error = %e, "Failed to create .matrix directory for logging");
        return;
    }

    let path = format!("{}/matrix-messages.jsonl", matrix_dir);
    let log_entry = MatrixMessageLog {
        timestamp: chrono::Utc::now().to_rfc3339(),
        room_id: room_id.to_string(),
        message_type: message_type.to_string(),
        content: content.to_string(),
        html: html.map(String::from),
        chunk_index,
        total_chunks,
    };

    let json_line = match serde_json::to_string(&log_entry) {
        Ok(j) => j,
        Err(e) => {
            tracing::warn!(error = %e, "Failed to serialize Matrix message log");
            return;
        }
    };

    match OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await
    {
        Ok(mut file) => {
            if let Err(e) = file.write_all(format!("{}\n", json_line).as_bytes()).await {
                tracing::warn!(error = %e, path = %path, "Failed to write Matrix message log");
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, path = %path, "Failed to open Matrix message log file");
        }
    }
}
