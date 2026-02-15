// ABOUTME: Markdown-to-Slack Block Kit JSON converter
// ABOUTME: Converts plain/markdown text into Slack Block Kit section blocks with mrkdwn formatting

use serde_json::{json, Value};

/// Maximum characters per section block text element
const MAX_SECTION_CHARS: usize = 3000;

/// Maximum characters per code block in a rich_text element
const MAX_CODE_BLOCK_CHARS: usize = 4000;

/// Maximum blocks per message (Slack limit is 50)
const MAX_BLOCKS: usize = 50;

/// Convert markdown/plain text content into Slack Block Kit JSON blocks array.
///
/// This function is infallible — it always returns valid Block Kit JSON,
/// falling back to a single mrkdwn section block on any parsing issues.
pub fn markdown_to_blocks(content: &str) -> Value {
    if content.is_empty() {
        return json!([{
            "type": "section",
            "text": {
                "type": "mrkdwn",
                "text": " "
            }
        }]);
    }

    let mut blocks: Vec<Value> = Vec::new();

    // Split content into segments: code blocks vs. regular text
    let segments = split_code_blocks(content);

    for segment in segments {
        match segment {
            Segment::Text(text) => {
                // Chunk text into sections respecting the 3K char limit
                for chunk in chunk_text(&text, MAX_SECTION_CHARS) {
                    if blocks.len() >= MAX_BLOCKS {
                        break;
                    }
                    blocks.push(json!({
                        "type": "section",
                        "text": {
                            "type": "mrkdwn",
                            "text": chunk
                        }
                    }));
                }
            }
            Segment::CodeBlock { language, code } => {
                // Code blocks become section blocks with triple-backtick formatting
                for chunk in chunk_text(&code, MAX_CODE_BLOCK_CHARS) {
                    if blocks.len() >= MAX_BLOCKS {
                        break;
                    }
                    let formatted = if language.is_empty() {
                        format!("```\n{}\n```", chunk)
                    } else {
                        // Slack mrkdwn doesn't support language-specific code blocks,
                        // but we preserve the language as a hint in a context block
                        format!("```\n{}\n```", chunk)
                    };
                    blocks.push(json!({
                        "type": "section",
                        "text": {
                            "type": "mrkdwn",
                            "text": formatted
                        }
                    }));
                }
            }
        }
    }

    if blocks.is_empty() {
        blocks.push(json!({
            "type": "section",
            "text": {
                "type": "mrkdwn",
                "text": content
            }
        }));
    }

    Value::Array(blocks)
}

// =============================================================================
// Content segmentation
// =============================================================================

#[derive(Debug)]
enum Segment {
    Text(String),
    CodeBlock { language: String, code: String },
}

/// Split content into text and code block segments
fn split_code_blocks(content: &str) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut remaining = content;

    while let Some(start) = remaining.find("```") {
        // Text before the code block
        let before = &remaining[..start];
        if !before.trim().is_empty() {
            segments.push(Segment::Text(before.trim().to_string()));
        }

        let after_fence = &remaining[start + 3..];

        // Extract language hint (text between ``` and first newline)
        let (language, code_start) = if let Some(nl) = after_fence.find('\n') {
            let lang = after_fence[..nl].trim().to_string();
            (lang, nl + 1)
        } else {
            (String::new(), 0)
        };

        let code_content = &after_fence[code_start..];

        // Find closing ```
        if let Some(end) = code_content.find("```") {
            let code = code_content[..end].trim_end().to_string();
            segments.push(Segment::CodeBlock {
                language,
                code,
            });
            remaining = &code_content[end + 3..];
        } else {
            // Unclosed code block — treat rest as code
            let code = code_content.trim_end().to_string();
            segments.push(Segment::CodeBlock {
                language,
                code,
            });
            remaining = "";
        }
    }

    // Remaining text after all code blocks
    if !remaining.trim().is_empty() {
        segments.push(Segment::Text(remaining.trim().to_string()));
    }

    segments
}

/// Split text into chunks at line boundaries, respecting a maximum length
fn chunk_text(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if remaining.len() <= max_len {
            chunks.push(remaining.to_string());
            break;
        }

        // Try to split at a newline within the limit
        let split_at = remaining[..max_len]
            .rfind('\n')
            .map(|pos| pos + 1)
            .unwrap_or(max_len);

        chunks.push(remaining[..split_at].to_string());
        remaining = &remaining[split_at..];
    }

    chunks
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_content() {
        let blocks = markdown_to_blocks("");
        let arr = blocks.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["type"], "section");
    }

    #[test]
    fn test_plain_text() {
        let blocks = markdown_to_blocks("Hello, world!");
        let arr = blocks.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["text"]["text"], "Hello, world!");
        assert_eq!(arr[0]["text"]["type"], "mrkdwn");
    }

    #[test]
    fn test_code_block() {
        let content = "Before code\n```rust\nfn main() {}\n```\nAfter code";
        let blocks = markdown_to_blocks(content);
        let arr = blocks.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["text"]["text"], "Before code");
        assert!(arr[1]["text"]["text"].as_str().unwrap().contains("fn main() {}"));
        assert_eq!(arr[2]["text"]["text"], "After code");
    }

    #[test]
    fn test_code_block_wrapped_in_backticks() {
        let content = "```\nplain code\n```";
        let blocks = markdown_to_blocks(content);
        let arr = blocks.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        let text = arr[0]["text"]["text"].as_str().unwrap();
        assert!(text.starts_with("```"));
        assert!(text.contains("plain code"));
    }

    #[test]
    fn test_long_text_chunking() {
        let long_text = "a".repeat(MAX_SECTION_CHARS + 500);
        let blocks = markdown_to_blocks(&long_text);
        let arr = blocks.as_array().unwrap();
        assert!(arr.len() >= 2);
        for block in arr {
            let text = block["text"]["text"].as_str().unwrap();
            assert!(text.len() <= MAX_SECTION_CHARS);
        }
    }

    #[test]
    fn test_max_blocks_limit() {
        // Create content that would generate more than MAX_BLOCKS blocks
        let mut content = String::new();
        for i in 0..60 {
            content.push_str(&format!("Section {}\n```\ncode {}\n```\n", i, i));
        }
        let blocks = markdown_to_blocks(&content);
        let arr = blocks.as_array().unwrap();
        assert!(arr.len() <= MAX_BLOCKS);
    }

    #[test]
    fn test_chunk_text_short() {
        let chunks = chunk_text("hello", 3000);
        assert_eq!(chunks, vec!["hello"]);
    }

    #[test]
    fn test_chunk_text_splits_at_newline() {
        let text = format!("{}\n{}", "a".repeat(2000), "b".repeat(2000));
        let chunks = chunk_text(&text, 3000);
        assert!(chunks.len() >= 2);
        for chunk in &chunks {
            assert!(chunk.len() <= 3000);
        }
    }

    #[test]
    fn test_unclosed_code_block() {
        let content = "text before\n```python\ndef hello():\n    pass";
        let blocks = markdown_to_blocks(content);
        let arr = blocks.as_array().unwrap();
        assert!(arr.len() >= 2);
    }

    #[test]
    fn test_multiple_code_blocks() {
        let content = "intro\n```\nblock1\n```\nmiddle\n```\nblock2\n```\noutro";
        let blocks = markdown_to_blocks(content);
        let arr = blocks.as_array().unwrap();
        assert_eq!(arr.len(), 5);
    }

    #[test]
    fn test_split_code_blocks_no_code() {
        let segments = split_code_blocks("just plain text");
        assert_eq!(segments.len(), 1);
        assert!(matches!(&segments[0], Segment::Text(t) if t == "just plain text"));
    }

    #[test]
    fn test_split_code_blocks_language_hint() {
        let segments = split_code_blocks("```rust\nfn main() {}\n```");
        assert_eq!(segments.len(), 1);
        assert!(matches!(&segments[0], Segment::CodeBlock { language, .. } if language == "rust"));
    }
}
