// ABOUTME: Mux backend - uses mux-rs for native Rust agent execution.
// ABOUTME: Provides streaming LLM responses with SQLite session persistence.

use crate::event::{AgentEvent, ErrorCode, Usage};
use crate::handle::{AgentHandle, Command};
use anyhow::{Context, Result};
use futures::StreamExt;
use mux::mcp::{McpClient, McpServerConfig, McpTransport};
use mux::prelude::*;
use mux::tool::Registry;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::{mpsc, RwLock};

use super::mux_tools::{
    WdBashTool, WdEditTool, WdListFilesTool, WdReadFileTool, WdSearchTool, WdWriteFileTool,
};
use mux::tools::{WebFetchTool, WebSearchTool};

/// Configuration for the Mux backend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MuxConfig {
    /// Model to use (e.g., "claude-sonnet-4-20250514")
    pub model: String,
    /// Maximum tokens for response
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    /// Working directory for the agent
    pub working_dir: PathBuf,
    /// Path to global system prompt file (e.g., ~/.mux/system.md)
    pub global_system_prompt_path: Option<PathBuf>,
    /// Filenames to look for local system prompts
    #[serde(default = "default_local_prompt_files")]
    pub local_prompt_files: Vec<String>,
    /// MCP servers to connect to
    #[serde(default)]
    pub mcp_servers: Vec<MuxMcpServerConfig>,
}

/// Configuration for an MCP server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MuxMcpServerConfig {
    /// Server name (used as tool prefix)
    pub name: String,
    /// Command to run
    pub command: String,
    /// Command arguments
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables
    #[serde(default)]
    pub env: HashMap<String, String>,
}

fn default_max_tokens() -> u32 {
    8192
}

fn default_local_prompt_files() -> Vec<String> {
    vec![
        "claude.md".to_string(),
        "CLAUDE.md".to_string(),
        "agent.md".to_string(),
    ]
}

/// Session state with message history
#[derive(Clone)]
struct MuxSession {
    messages: Vec<Message>,
    system_prompt: Option<String>,
}

impl MuxSession {
    fn new(system_prompt: Option<String>) -> Self {
        Self {
            messages: Vec::new(),
            system_prompt,
        }
    }
}

/// Serializable message format for SQLite storage
#[derive(Serialize, Deserialize)]
struct StoredMessage {
    role: String,
    content: Vec<StoredContentBlock>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
enum StoredContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
}

/// SQLite-backed session store for mux backend
struct SessionDb {
    conn: Mutex<Connection>,
}

impl SessionDb {
    fn new(db_path: &std::path::Path) -> Result<Self> {
        let conn = Connection::open(db_path).context("Failed to open mux sessions database")?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS mux_sessions (
                session_id TEXT PRIMARY KEY,
                messages_json TEXT NOT NULL,
                system_prompt TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
            [],
        )?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn save_session(&self, session_id: &str, session: &MuxSession) -> Result<()> {
        let messages: Vec<StoredMessage> = session
            .messages
            .iter()
            .map(|m| StoredMessage {
                role: match m.role {
                    Role::User => "user".to_string(),
                    Role::Assistant => "assistant".to_string(),
                },
                content: m
                    .content
                    .iter()
                    .filter_map(|c| match c {
                        ContentBlock::Text { text } => {
                            Some(StoredContentBlock::Text { text: text.clone() })
                        }
                        ContentBlock::ToolUse { id, name, input } => {
                            Some(StoredContentBlock::ToolUse {
                                id: id.clone(),
                                name: name.clone(),
                                input: input.clone(),
                            })
                        }
                        ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                        } => Some(StoredContentBlock::ToolResult {
                            tool_use_id: tool_use_id.clone(),
                            content: content.clone(),
                            is_error: *is_error,
                        }),
                    })
                    .collect(),
            })
            .collect();

        let messages_json = serde_json::to_string(&messages)?;
        let now = chrono::Utc::now().to_rfc3339();

        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;

        conn.execute(
            "INSERT INTO mux_sessions (session_id, messages_json, system_prompt, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(session_id) DO UPDATE SET
                messages_json = ?2,
                system_prompt = ?3,
                updated_at = ?4",
            params![session_id, messages_json, session.system_prompt, now],
        )?;

        tracing::debug!(session_id = %session_id, messages = messages.len(), "Session saved to database");
        Ok(())
    }

    fn load_session(&self, session_id: &str) -> Result<Option<MuxSession>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;

        let mut stmt = conn.prepare(
            "SELECT messages_json, system_prompt FROM mux_sessions WHERE session_id = ?1",
        )?;

        let result = stmt.query_row(params![session_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        });

        match result {
            Ok((messages_json, system_prompt)) => {
                let stored: Vec<StoredMessage> = serde_json::from_str(&messages_json)?;
                let messages: Vec<Message> = stored
                    .into_iter()
                    .map(|m| Message {
                        role: if m.role == "user" {
                            Role::User
                        } else {
                            Role::Assistant
                        },
                        content: m
                            .content
                            .into_iter()
                            .map(|c| match c {
                                StoredContentBlock::Text { text } => ContentBlock::Text { text },
                                StoredContentBlock::ToolUse { id, name, input } => {
                                    ContentBlock::ToolUse { id, name, input }
                                }
                                StoredContentBlock::ToolResult {
                                    tool_use_id,
                                    content,
                                    is_error,
                                } => ContentBlock::ToolResult {
                                    tool_use_id,
                                    content,
                                    is_error,
                                },
                            })
                            .collect(),
                    })
                    .collect();

                tracing::debug!(session_id = %session_id, messages = messages.len(), "Session loaded from database");
                Ok(Some(MuxSession {
                    messages,
                    system_prompt,
                }))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

pub struct MuxBackend {
    config: MuxConfig,
    additional_tools: Vec<Box<dyn mux::tool::Tool>>,
}

/// Read MCP server configs from .mcp.json in the working directory
fn read_mcp_json(working_dir: &std::path::Path) -> Vec<MuxMcpServerConfig> {
    let mcp_path = working_dir.join(".mcp.json");
    if !mcp_path.exists() {
        return Vec::new();
    }

    let content = match std::fs::read_to_string(&mcp_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(path = %mcp_path.display(), error = %e, "Failed to read .mcp.json");
            return Vec::new();
        }
    };

    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(path = %mcp_path.display(), error = %e, "Failed to parse .mcp.json");
            return Vec::new();
        }
    };

    // Handle Claude Code format: { "mcpServers": { ... } }
    let servers_obj = if let Some(mcp_servers) = json.get("mcpServers") {
        mcp_servers.as_object()
    } else {
        // Also try direct format: { "server-name": { ... } }
        json.as_object()
    };

    let Some(servers) = servers_obj else {
        tracing::warn!(path = %mcp_path.display(), "Invalid .mcp.json format");
        return Vec::new();
    };

    let mut configs = Vec::new();
    for (name, server) in servers {
        // Skip non-stdio servers (http type not supported yet)
        if server.get("type").and_then(|t| t.as_str()) == Some("http") {
            tracing::debug!(server = %name, "Skipping HTTP MCP server (not supported)");
            continue;
        }

        let Some(command) = server.get("command").and_then(|c| c.as_str()) else {
            continue;
        };

        let args: Vec<String> = server
            .get("args")
            .and_then(|a| a.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let env: HashMap<String, String> = server
            .get("env")
            .and_then(|e| e.as_object())
            .map(|obj| {
                obj.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();

        configs.push(MuxMcpServerConfig {
            name: name.clone(),
            command: command.to_string(),
            args,
            env,
        });

        tracing::info!(server = %name, command = %command, "Found MCP server in .mcp.json");
    }

    configs
}

impl MuxBackend {
    pub fn new(config: MuxConfig) -> Result<Self> {
        Ok(Self {
            config,
            additional_tools: Vec::new(),
        })
    }

    /// Add custom tools to be registered with this backend
    pub fn with_tools(mut self, tools: Vec<Box<dyn mux::tool::Tool>>) -> Self {
        self.additional_tools = tools;
        self
    }

    pub fn into_handle(self) -> AgentHandle {
        let (tx, mut rx) = mpsc::channel::<Command>(32);
        let name = "mux";
        let config = self.config;

        // Create the Anthropic client
        let client = match AnthropicClient::from_env() {
            Ok(c) => Arc::new(c),
            Err(e) => {
                tracing::error!(error = %e, "Failed to create Anthropic client");
                return AgentHandle::new(tx, name);
            }
        };

        // Create session database in working directory
        let db_path = config.working_dir.join(".mux_sessions.db");
        let session_db = match SessionDb::new(&db_path) {
            Ok(db) => Arc::new(db),
            Err(e) => {
                tracing::error!(error = %e, "Failed to create session database");
                return AgentHandle::new(tx, name);
            }
        };

        // Create tool registry
        let registry = Arc::new(Registry::new());

        // In-memory session cache (backed by SQLite)
        let sessions: Arc<RwLock<HashMap<String, MuxSession>>> =
            Arc::new(RwLock::new(HashMap::new()));

        // Store MCP clients for shutdown
        let mcp_clients: Arc<RwLock<Vec<Arc<McpClient>>>> = Arc::new(RwLock::new(Vec::new()));

        // Connect to MCP servers in background
        // Merge servers from config with servers from .mcp.json in working directory
        let registry_clone = Arc::clone(&registry);
        let mcp_clients_clone = Arc::clone(&mcp_clients);
        let mut mcp_configs = config.mcp_servers.clone();
        let json_configs = read_mcp_json(&config.working_dir);
        mcp_configs.extend(json_configs);

        tracing::info!(
            config_servers = config.mcp_servers.len(),
            json_servers = mcp_configs.len() - config.mcp_servers.len(),
            total = mcp_configs.len(),
            "Loading MCP servers"
        );

        // Connect MCP servers in background (can be slow, don't block)
        tokio::spawn(async move {
            for server_config in mcp_configs {
                match connect_mcp_server(&server_config, &registry_clone).await {
                    Ok(client) => {
                        mcp_clients_clone.write().await.push(client);
                        tracing::info!(server = %server_config.name, "MCP server connected");
                    }
                    Err(e) => {
                        tracing::error!(
                            server = %server_config.name,
                            error = %e,
                            "Failed to connect MCP server"
                        );
                    }
                }
            }
        });

        // Clone registry for command loop (tools will be registered here FIRST)
        let registry_for_loop = Arc::clone(&registry);
        let working_dir_for_tools = config.working_dir.clone();
        let additional_tools = self.additional_tools;

        tokio::spawn(async move {
            // CRITICAL: Register all tools BEFORE processing any commands
            // This fixes the race condition where prompts were sent before tools were ready
            let wd = working_dir_for_tools;

            // 1. read_file - Read file contents
            registry_for_loop
                .register(WdReadFileTool::new(wd.clone()))
                .await;
            // 2. write_file - Write/create files
            registry_for_loop
                .register(WdWriteFileTool::new(wd.clone()))
                .await;
            // 3. edit - Precise string replacement
            registry_for_loop
                .register(WdEditTool::new(wd.clone()))
                .await;
            // 4. bash - Execute shell commands
            registry_for_loop
                .register(WdBashTool::new(wd.clone()))
                .await;
            // 5. list_files - List directory contents
            registry_for_loop
                .register(WdListFilesTool::new(wd.clone()))
                .await;
            // 6. search - Search file contents (grep-like)
            registry_for_loop
                .register(WdSearchTool::new(wd.clone()))
                .await;
            // 7. web_fetch - Fetch URL content
            registry_for_loop.register(WebFetchTool::new()).await;
            // 8. web_search - Web search queries
            registry_for_loop.register(WebSearchTool::new()).await;

            tracing::info!(
                working_dir = %wd.display(),
                "Registered 8 built-in tools: read_file, write_file, edit, bash, list_files, search, web_fetch, web_search"
            );

            // Register additional custom tools (e.g., DISPATCH tools)
            let additional_count = additional_tools.len();
            for tool in additional_tools {
                let tool_name = tool.name().to_string();
                // Convert Box<dyn Tool> to Arc<dyn Tool>
                let arc_tool: Arc<dyn mux::tool::Tool> = Arc::from(tool);
                registry_for_loop.register_arc(arc_tool).await;
                tracing::debug!(tool = %tool_name, "Registered custom tool");
            }
            if additional_count > 0 {
                tracing::info!(
                    count = additional_count,
                    "Registered additional custom tools"
                );
            }

            // NOW process commands (tools are guaranteed to be registered)
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    Command::NewSession { reply } => {
                        let session_id = uuid::Uuid::new_v4().to_string();
                        let system_prompt = build_system_prompt(&config);
                        let session = MuxSession::new(system_prompt);

                        // Save to database immediately
                        if let Err(e) = session_db.save_session(&session_id, &session) {
                            tracing::error!(error = %e, "Failed to save new session");
                        }

                        sessions.write().await.insert(session_id.clone(), session);
                        let _ = reply.send(Ok(session_id));
                    }
                    Command::LoadSession {
                        session_id, reply, ..
                    } => {
                        // Check in-memory cache first
                        let exists = sessions.read().await.contains_key(&session_id);
                        if exists {
                            let _ = reply.send(Ok(()));
                            continue;
                        }

                        // Try to load from database
                        match session_db.load_session(&session_id) {
                            Ok(Some(session)) => {
                                sessions.write().await.insert(session_id.clone(), session);
                                let _ = reply.send(Ok(()));
                            }
                            Ok(None) => {
                                // Session doesn't exist - create new one
                                let system_prompt = build_system_prompt(&config);
                                let session = MuxSession::new(system_prompt);
                                if let Err(e) = session_db.save_session(&session_id, &session) {
                                    tracing::error!(error = %e, "Failed to save new session");
                                }
                                sessions.write().await.insert(session_id.clone(), session);
                                let _ = reply.send(Ok(()));
                            }
                            Err(e) => {
                                tracing::error!(error = %e, "Failed to load session");
                                let _ = reply.send(Err(e));
                            }
                        }
                    }
                    Command::Prompt {
                        session_id,
                        text,
                        event_tx,
                        reply,
                        ..
                    } => {
                        let _ = reply.send(Ok(()));

                        let client = Arc::clone(&client);
                        let sessions = Arc::clone(&sessions);
                        let session_db = Arc::clone(&session_db);
                        let registry = Arc::clone(&registry);
                        let config = config.clone();

                        tokio::spawn(async move {
                            if let Err(e) = run_prompt(
                                &client,
                                &sessions,
                                &session_db,
                                &registry,
                                &config,
                                &session_id,
                                &text,
                                event_tx,
                            )
                            .await
                            {
                                tracing::error!(error = %e, "Mux prompt failed");
                            }
                        });
                    }
                    Command::Cancel { reply, .. } => {
                        let _ = reply.send(Ok(()));
                    }
                }
            }
        });

        AgentHandle::new(tx, name)
    }

    /// Factory function for the registry
    pub fn factory() -> crate::registry::BackendFactory {
        Box::new(|config| {
            let cfg: MuxConfig = serde_json::from_value(config.clone())?;
            let backend = MuxBackend::new(cfg)?;
            Ok(backend.into_handle())
        })
    }
}

/// Build system prompt from global and local files
fn build_system_prompt(config: &MuxConfig) -> Option<String> {
    let mut parts = Vec::new();

    // 0. Working directory context - critical for tools to work correctly
    let working_dir_str = config.working_dir.to_string_lossy();
    parts.push(format!(
        "# Environment\n\n\
        Your working directory is: {}\n\n\
        When using file tools (read_file, write_file, list_files, search), paths are relative to this directory.\n\
        When using the bash tool, pass working_dir: \"{}\" unless you need to run in a different directory.",
        working_dir_str, working_dir_str
    ));

    // 1. Global system prompt (~/.mux/system.md or configured path)
    if let Some(ref global_path) = config.global_system_prompt_path {
        if let Ok(content) = std::fs::read_to_string(global_path) {
            if !content.trim().is_empty() {
                parts.push(content);
            }
        }
    } else {
        // Default to ~/.mux/system.md
        if let Some(home) = dirs::home_dir() {
            let default_path = home.join(".mux").join("system.md");
            if let Ok(content) = std::fs::read_to_string(default_path) {
                if !content.trim().is_empty() {
                    parts.push(content);
                }
            }
        }
    }

    // 2. Local system prompt (claude.md, agent.md, etc. in working_dir)
    for filename in &config.local_prompt_files {
        let local_path = config.working_dir.join(filename);
        if let Ok(content) = std::fs::read_to_string(&local_path) {
            if !content.trim().is_empty() {
                parts.push(content);
                break;
            }
        }
    }

    // Always have at least the working directory context
    Some(parts.join("\n\n---\n\n"))
}

/// Connect to an MCP server and register its tools
async fn connect_mcp_server(
    config: &MuxMcpServerConfig,
    registry: &Registry,
) -> Result<Arc<McpClient>> {
    let mcp_config = McpServerConfig {
        name: config.name.clone(),
        transport: McpTransport::Stdio {
            command: config.command.clone(),
            args: config.args.clone(),
            env: config.env.clone(),
        },
    };

    let mut client = McpClient::connect(mcp_config).await?;
    client.initialize().await?;

    let client = Arc::new(client);
    let tool_count = registry
        .merge_mcp(Arc::clone(&client), Some(&config.name))
        .await?;

    tracing::info!(
        server = %config.name,
        tools = tool_count,
        "Registered MCP tools"
    );

    Ok(client)
}

async fn run_prompt(
    client: &AnthropicClient,
    sessions: &Arc<RwLock<HashMap<String, MuxSession>>>,
    session_db: &Arc<SessionDb>,
    registry: &Registry,
    config: &MuxConfig,
    session_id: &str,
    text: &str,
    event_tx: mpsc::Sender<AgentEvent>,
) -> Result<()> {
    // Get tool definitions from registry
    let tools = registry.to_definitions().await;

    // Get session and add user message
    let system_prompt = {
        let mut sessions_guard = sessions.write().await;
        let session = sessions_guard
            .get_mut(session_id)
            .context("Session not found")?;

        session.messages.push(Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
        });

        session.system_prompt.clone()
    };

    let mut accumulated_text = String::new();
    let mut total_usage = Usage {
        input_tokens: 0,
        output_tokens: 0,
        cache_read_tokens: None,
        cache_write_tokens: None,
        cost_usd: None,
        extra: None,
    };

    // Agentic loop - continues while LLM requests tool use
    loop {
        // Get current messages from session
        let messages = {
            let sessions_guard = sessions.read().await;
            let session = sessions_guard
                .get(session_id)
                .context("Session not found")?;
            session.messages.clone()
        };

        // Build request
        let request = Request {
            model: config.model.clone(),
            messages,
            tools: tools.clone(),
            max_tokens: Some(config.max_tokens),
            system: system_prompt.clone(),
            temperature: None,
        };

        // Use streaming API for real-time text output
        let mut stream = client.create_message_stream(&request);
        let mut response_content: Vec<ContentBlock> = Vec::new();
        let mut current_text = String::new();
        let mut tool_uses: Vec<(String, String, serde_json::Value)> = Vec::new();
        let mut stop_reason: Option<StopReason> = None;

        // Track tool input JSON accumulation by block index
        let mut tool_input_accum: std::collections::HashMap<usize, String> =
            std::collections::HashMap::new();
        let mut tool_index_map: std::collections::HashMap<usize, usize> =
            std::collections::HashMap::new(); // block index -> tool_uses index

        while let Some(event_result) = stream.next().await {
            match event_result {
                Ok(event) => {
                    match &event {
                        StreamEvent::ContentBlockDelta { text, .. } => {
                            current_text.push_str(text);
                            accumulated_text.push_str(text);
                            if event_tx.send(AgentEvent::Text(text.clone())).await.is_err() {
                                tracing::debug!("Event receiver closed, stopping stream");
                                return Ok(());
                            }
                        }
                        StreamEvent::InputJsonDelta {
                            index,
                            partial_json,
                        } => {
                            // Accumulate tool input JSON fragments
                            tool_input_accum
                                .entry(*index)
                                .or_default()
                                .push_str(partial_json);
                        }
                        StreamEvent::ContentBlockStart { index, block } => {
                            if let ContentBlock::ToolUse { id, name, input } = block {
                                // Emit ToolStart event (input may be empty at this point)
                                let _ = event_tx
                                    .send(AgentEvent::ToolStart {
                                        id: id.clone(),
                                        name: name.clone(),
                                        input: input.clone(),
                                    })
                                    .await;
                                // Track mapping from block index to tool_uses index
                                tool_index_map.insert(*index, tool_uses.len());
                                tool_uses.push((id.clone(), name.clone(), input.clone()));
                            }
                        }
                        StreamEvent::ContentBlockStop { index } => {
                            // Finalize text blocks
                            if !current_text.is_empty() && response_content.len() == *index as usize
                            {
                                response_content.push(ContentBlock::Text {
                                    text: std::mem::take(&mut current_text),
                                });
                            }
                            // Finalize tool input JSON if this was a tool block
                            if let Some(tool_idx) = tool_index_map.get(index) {
                                if let Some(json_str) = tool_input_accum.remove(index) {
                                    if let Ok(parsed) =
                                        serde_json::from_str::<serde_json::Value>(&json_str)
                                    {
                                        // Update the tool input with the accumulated JSON
                                        if let Some(tool) = tool_uses.get_mut(*tool_idx) {
                                            tool.2 = parsed;
                                        }
                                    }
                                }
                            }
                        }
                        StreamEvent::MessageDelta {
                            usage,
                            stop_reason: sr,
                            ..
                        } => {
                            total_usage.input_tokens += usage.input_tokens as u64;
                            total_usage.output_tokens += usage.output_tokens as u64;
                            stop_reason = sr.clone();
                        }
                        StreamEvent::MessageStart { .. } | StreamEvent::MessageStop => {}
                    }
                }
                Err(e) => {
                    let error_event = translate_llm_error(&e);
                    let _ = event_tx.send(error_event).await;
                    return Err(e.into());
                }
            }
        }

        // Add any remaining text
        if !current_text.is_empty() {
            response_content.push(ContentBlock::Text { text: current_text });
        }

        // Add tool uses to content
        for (id, name, input) in &tool_uses {
            response_content.push(ContentBlock::ToolUse {
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
            });
        }

        // Update session with assistant response
        {
            let mut sessions_guard = sessions.write().await;
            if let Some(session) = sessions_guard.get_mut(session_id) {
                if !response_content.is_empty() {
                    session.messages.push(Message {
                        role: Role::Assistant,
                        content: response_content.clone(),
                    });
                }
            }
        }

        // If no tool use, we're done
        if stop_reason != Some(StopReason::ToolUse) || tool_uses.is_empty() {
            // Persist final state
            {
                let sessions_guard = sessions.read().await;
                if let Some(session) = sessions_guard.get(session_id) {
                    if let Err(e) = session_db.save_session(session_id, session) {
                        tracing::error!(error = %e, "Failed to persist session after prompt");
                    }
                }
            }
            break;
        }

        // Execute tools and collect results
        let mut tool_results: Vec<ContentBlock> = Vec::new();

        for (tool_id, tool_name, tool_input) in tool_uses {
            let start_time = Instant::now();

            // Look up and execute the tool
            let (output, is_error) = if let Some(tool) = registry.get(&tool_name).await {
                match tool.execute(tool_input.clone()).await {
                    Ok(result) => (result.content, result.is_error),
                    Err(e) => (format!("Tool execution error: {}", e), true),
                }
            } else {
                (format!("Tool '{}' not found in registry", tool_name), true)
            };

            let duration_ms = start_time.elapsed().as_millis() as u64;

            // Emit ToolEnd event
            let _ = event_tx
                .send(AgentEvent::ToolEnd {
                    id: tool_id.clone(),
                    name: tool_name.clone(),
                    output: serde_json::Value::String(output.clone()),
                    success: !is_error,
                    duration_ms,
                })
                .await;

            tool_results.push(ContentBlock::ToolResult {
                tool_use_id: tool_id,
                content: output,
                is_error,
            });
        }

        // Add tool results as a user message and persist
        {
            let mut sessions_guard = sessions.write().await;
            if let Some(session) = sessions_guard.get_mut(session_id) {
                session.messages.push(Message {
                    role: Role::User,
                    content: tool_results,
                });

                // Persist after tool execution
                if let Err(e) = session_db.save_session(session_id, session) {
                    tracing::error!(error = %e, "Failed to persist session after tool execution");
                }
            }
        }

        // Continue the loop to let LLM respond to tool results
    }

    // Send final result
    let _ = event_tx
        .send(AgentEvent::Result {
            text: accumulated_text,
            usage: Some(total_usage),
            metadata: serde_json::Value::Null,
        })
        .await;

    Ok(())
}

/// Translate mux LlmError to gorp AgentEvent
fn translate_llm_error(error: &LlmError) -> AgentEvent {
    let (code, message, recoverable) = match error {
        LlmError::Http(e) => (ErrorCode::BackendError, e.to_string(), true),
        LlmError::Api { message, .. } => {
            let code = if message.contains("rate_limit") {
                ErrorCode::RateLimited
            } else if message.contains("authentication") || message.contains("invalid_api_key") {
                ErrorCode::AuthFailed
            } else {
                ErrorCode::BackendError
            };
            (code, message.clone(), code == ErrorCode::RateLimited)
        }
        LlmError::StreamClosed => (
            ErrorCode::BackendError,
            "Stream closed unexpectedly".to_string(),
            false,
        ),
        _ => (ErrorCode::BackendError, error.to_string(), false),
    };

    AgentEvent::Error {
        code,
        message,
        recoverable,
    }
}

mod dirs {
    pub fn home_dir() -> Option<std::path::PathBuf> {
        std::env::var_os("HOME").map(std::path::PathBuf::from)
    }
}

mod chrono {
    pub struct Utc;
    impl Utc {
        pub fn now() -> DateTime {
            DateTime
        }
    }
    pub struct DateTime;
    impl DateTime {
        pub fn to_rfc3339(&self) -> String {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap();
            format!("{}", now.as_secs())
        }
    }
}
