// ABOUTME: Context file and dispatch event routing
// ABOUTME: MCP context files and DISPATCH control plane event handling

use anyhow::Result;
use std::path::Path;

use crate::session::SessionStore;

/// Write context file for MCP tools to read
/// This tells tools like gorp_schedule_prompt which channel/room they're operating in
pub async fn write_context_file(
    channel_dir: &str,
    room_id: &str,
    channel_name: &str,
    session_id: &str,
) -> Result<()> {
    let gorp_dir = Path::new(channel_dir).join(".gorp");
    tokio::fs::create_dir_all(&gorp_dir).await?;

    let context = serde_json::json!({
        "room_id": room_id,
        "channel_name": channel_name,
        "session_id": session_id,
        "updated_at": chrono::Utc::now().to_rfc3339()
    });

    let context_path = gorp_dir.join("context.json");
    tokio::fs::write(&context_path, serde_json::to_string_pretty(&context)?).await?;

    tracing::debug!(path = %context_path.display(), "Wrote MCP context file");
    Ok(())
}

/// Route an agent event to the DISPATCH control plane
///
/// When an agent emits a custom event with a "dispatch:" prefix,
/// this function stores it in the dispatch_events table for
/// later processing by the DISPATCH agent.
pub async fn route_to_dispatch(
    session_store: &SessionStore,
    source_room_id: &str,
    event_kind: &str,
    payload: &serde_json::Value,
) -> Result<()> {
    // Create dispatch event for storage
    let event = crate::session::DispatchEvent {
        id: uuid::Uuid::new_v4().to_string(),
        source_room_id: source_room_id.to_string(),
        event_type: event_kind.to_string(),
        payload: payload.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
        acknowledged_at: None,
    };

    session_store.insert_dispatch_event(&event)?;
    tracing::info!(
        event_id = %event.id,
        event_type = %event_kind,
        source_room = %source_room_id,
        "Event queued for DISPATCH"
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_write_context_file() {
        let temp_dir = TempDir::new().unwrap();
        let channel_dir = temp_dir.path().to_str().unwrap();

        let result = write_context_file(
            channel_dir,
            "!test:matrix.org",
            "test-channel",
            "session-123",
        )
        .await;

        assert!(result.is_ok());

        // Verify file was created
        let context_path = temp_dir.path().join(".gorp").join("context.json");
        assert!(context_path.exists());

        // Verify content
        let content = std::fs::read_to_string(&context_path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(json["room_id"], "!test:matrix.org");
        assert_eq!(json["channel_name"], "test-channel");
        assert_eq!(json["session_id"], "session-123");
    }
}
