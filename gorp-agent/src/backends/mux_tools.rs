// ABOUTME: Working directory-aware wrapper tools for mux backend.
// ABOUTME: Resolves relative paths against the channel's working directory.

use async_trait::async_trait;
use mux::tool::{Tool, ToolResult};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// Helper to resolve a path against the working directory.
/// Absolute paths are returned as-is, relative paths are joined with working_dir.
fn resolve_path(working_dir: &Path, path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        working_dir.join(p)
    }
}

/// ReadFileTool with working directory support.
pub struct WdReadFileTool {
    working_dir: PathBuf,
}

impl WdReadFileTool {
    pub fn new(working_dir: PathBuf) -> Self {
        Self { working_dir }
    }
}

#[async_trait]
impl Tool for WdReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read the contents of a file. Relative paths are resolved from your working directory."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The path to the file to read (relative to working directory or absolute)"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, params: serde_json::Value) -> Result<ToolResult, anyhow::Error> {
        #[derive(Deserialize)]
        struct Params {
            path: String,
        }
        let params: Params = serde_json::from_value(params)?;
        let resolved = resolve_path(&self.working_dir, &params.path);

        match std::fs::read_to_string(&resolved) {
            Ok(content) => Ok(ToolResult::text(content)),
            Err(e) => Ok(ToolResult::error(format!(
                "Failed to read file '{}': {}",
                resolved.display(),
                e
            ))),
        }
    }
}

/// WriteFileTool with working directory support.
pub struct WdWriteFileTool {
    working_dir: PathBuf,
}

impl WdWriteFileTool {
    pub fn new(working_dir: PathBuf) -> Self {
        Self { working_dir }
    }
}

#[async_trait]
impl Tool for WdWriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write content to a file. Creates parent directories if needed. Relative paths are resolved from your working directory."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The path to the file to write (relative to working directory or absolute)"
                },
                "content": {
                    "type": "string",
                    "description": "The content to write to the file"
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, params: serde_json::Value) -> Result<ToolResult, anyhow::Error> {
        #[derive(Deserialize)]
        struct Params {
            path: String,
            content: String,
        }
        let params: Params = serde_json::from_value(params)?;
        let resolved = resolve_path(&self.working_dir, &params.path);

        // Create parent directories if needed
        if let Some(parent) = resolved.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }

        match std::fs::write(&resolved, &params.content) {
            Ok(()) => Ok(ToolResult::text(format!(
                "Successfully wrote {} bytes to {}",
                params.content.len(),
                resolved.display()
            ))),
            Err(e) => Ok(ToolResult::error(format!(
                "Failed to write file '{}': {}",
                resolved.display(),
                e
            ))),
        }
    }
}

/// ListFilesTool with working directory support.
pub struct WdListFilesTool {
    working_dir: PathBuf,
}

impl WdListFilesTool {
    pub fn new(working_dir: PathBuf) -> Self {
        Self { working_dir }
    }
}

#[async_trait]
impl Tool for WdListFilesTool {
    fn name(&self) -> &str {
        "list_files"
    }

    fn description(&self) -> &str {
        "List files in a directory matching a glob pattern. Defaults to your working directory."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The directory to list (default: working directory)"
                },
                "glob": {
                    "type": "string",
                    "description": "Glob pattern to match (default: *)"
                }
            }
        })
    }

    async fn execute(&self, params: serde_json::Value) -> Result<ToolResult, anyhow::Error> {
        #[derive(Deserialize, Default)]
        struct Params {
            path: Option<String>,
            glob: Option<String>,
        }
        let params: Params = serde_json::from_value(params).unwrap_or_default();

        // Use working_dir as default if no path specified
        let base_path = match &params.path {
            Some(p) => resolve_path(&self.working_dir, p),
            None => self.working_dir.clone(),
        };
        let glob_pattern = params.glob.unwrap_or_else(|| "*".to_string());
        let full_pattern = format!("{}/{}", base_path.display(), glob_pattern);

        let mut files = Vec::new();
        for entry in glob::glob(&full_pattern).unwrap_or_else(|_| glob::glob("").unwrap()) {
            if let Ok(path) = entry {
                // Show paths relative to working_dir for cleaner output
                let display_path = path
                    .strip_prefix(&self.working_dir)
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| path.display().to_string());
                let prefix = if path.is_dir() { "[dir] " } else { "" };
                files.push(format!("{}{}", prefix, display_path));
            }
        }

        if files.is_empty() {
            Ok(ToolResult::text("No files found"))
        } else {
            Ok(ToolResult::text(files.join("\n")))
        }
    }
}

/// SearchTool with working directory support.
pub struct WdSearchTool {
    working_dir: PathBuf,
}

impl WdSearchTool {
    pub fn new(working_dir: PathBuf) -> Self {
        Self { working_dir }
    }
}

#[async_trait]
impl Tool for WdSearchTool {
    fn name(&self) -> &str {
        "search"
    }

    fn description(&self) -> &str {
        "Search for a pattern in files. Supports glob patterns for file matching and regex for content matching. Defaults to your working directory."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "The regex pattern to search for in file contents"
                },
                "path": {
                    "type": "string",
                    "description": "The directory to search in (default: working directory)"
                },
                "glob": {
                    "type": "string",
                    "description": "Glob pattern for files to search (default: **/*)"
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, params: serde_json::Value) -> Result<ToolResult, anyhow::Error> {
        #[derive(Deserialize)]
        struct Params {
            pattern: String,
            path: Option<String>,
            glob: Option<String>,
        }
        let params: Params = serde_json::from_value(params)?;

        let base_path = match &params.path {
            Some(p) => resolve_path(&self.working_dir, p),
            None => self.working_dir.clone(),
        };
        let glob_pattern = params.glob.unwrap_or_else(|| "**/*".to_string());
        let full_pattern = format!("{}/{}", base_path.display(), glob_pattern);

        let mut results = Vec::new();
        let regex = match regex::Regex::new(&params.pattern) {
            Ok(r) => r,
            Err(e) => return Ok(ToolResult::error(format!("Invalid regex: {}", e))),
        };

        for entry in glob::glob(&full_pattern).unwrap_or_else(|_| glob::glob("").unwrap()) {
            if let Ok(path) = entry {
                if path.is_file() {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        for (line_num, line) in content.lines().enumerate() {
                            if regex.is_match(line) {
                                // Show paths relative to working_dir
                                let display_path = path
                                    .strip_prefix(&self.working_dir)
                                    .map(|p| p.display().to_string())
                                    .unwrap_or_else(|_| path.display().to_string());
                                results.push(format!(
                                    "{}:{}: {}",
                                    display_path,
                                    line_num + 1,
                                    line.trim()
                                ));
                            }
                        }
                    }
                }
            }
        }

        if results.is_empty() {
            Ok(ToolResult::text("No matches found"))
        } else {
            Ok(ToolResult::text(format!(
                "Found {} matches:\n{}",
                results.len(),
                results.join("\n")
            )))
        }
    }
}

/// BashTool with working directory default.
pub struct WdBashTool {
    working_dir: PathBuf,
}

impl WdBashTool {
    pub fn new(working_dir: PathBuf) -> Self {
        Self { working_dir }
    }
}

#[async_trait]
impl Tool for WdBashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Execute a bash command. Commands run in your working directory by default."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The bash command to execute"
                },
                "working_dir": {
                    "type": "string",
                    "description": "Override the working directory for this command (default: your working directory)"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, params: serde_json::Value) -> Result<ToolResult, anyhow::Error> {
        #[derive(Deserialize)]
        struct Params {
            command: String,
            working_dir: Option<String>,
        }
        let params: Params = serde_json::from_value(params)?;

        let mut cmd = tokio::process::Command::new("bash");
        cmd.arg("-c").arg(&params.command);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        // Use provided working_dir or default to our working_dir
        let cwd = match &params.working_dir {
            Some(dir) => resolve_path(&self.working_dir, dir),
            None => self.working_dir.clone(),
        };
        cmd.current_dir(&cwd);

        let output = cmd.output().await?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let result = if output.status.success() {
            if stderr.is_empty() {
                stdout.to_string()
            } else {
                format!("{}\n\nstderr:\n{}", stdout, stderr)
            }
        } else {
            format!(
                "Command failed with exit code {}\n\nstdout:\n{}\n\nstderr:\n{}",
                output.status.code().unwrap_or(-1),
                stdout,
                stderr
            )
        };

        if output.status.success() {
            Ok(ToolResult::text(result))
        } else {
            Ok(ToolResult::error(result))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_read_file_relative() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("test.txt"), "Hello!").unwrap();

        let tool = WdReadFileTool::new(dir.path().to_path_buf());
        let result = tool
            .execute(serde_json::json!({"path": "test.txt"}))
            .await
            .unwrap();

        assert!(!result.is_error);
        assert_eq!(result.content, "Hello!");
    }

    #[tokio::test]
    async fn test_write_file_relative() {
        let dir = TempDir::new().unwrap();

        let tool = WdWriteFileTool::new(dir.path().to_path_buf());
        let result = tool
            .execute(serde_json::json!({"path": "test.txt", "content": "Hello!"}))
            .await
            .unwrap();

        assert!(!result.is_error);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("test.txt")).unwrap(),
            "Hello!"
        );
    }

    #[tokio::test]
    async fn test_bash_uses_working_dir() {
        let dir = TempDir::new().unwrap();

        let tool = WdBashTool::new(dir.path().to_path_buf());
        let result = tool
            .execute(serde_json::json!({"command": "pwd"}))
            .await
            .unwrap();

        assert!(!result.is_error);
        // The output should contain the temp dir path
        assert!(
            result.content.contains(dir.path().to_str().unwrap())
                || result.content.contains("private")
        ); // macOS /tmp -> /private/tmp
    }
}
