// ABOUTME: Attachment handling for Matrix messages
// ABOUTME: Downloads images and files from Matrix media server to workspace

use anyhow::Result;
use matrix_sdk::{
    media::{MediaFormat, MediaRequestParameters},
    Client,
};
use std::path::Path;

/// Download an attachment from Matrix and save it to the workspace
/// Returns the relative path to the saved file
pub async fn download_attachment(
    client: &Client,
    source: &matrix_sdk::ruma::events::room::MediaSource,
    filename: &str,
    workspace_dir: &str,
) -> Result<String> {
    use tokio::io::AsyncWriteExt;

    // Create attachments directory
    let attachments_dir = Path::new(workspace_dir).join("attachments");
    tokio::fs::create_dir_all(&attachments_dir).await?;

    // Generate unique filename to avoid collisions
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    let safe_filename = sanitize_filename(filename);
    let unique_filename = format!("{}_{}", timestamp, safe_filename);
    let file_path = attachments_dir.join(&unique_filename);

    // Download the media
    let request = MediaRequestParameters {
        source: source.clone(),
        format: MediaFormat::File,
    };

    let data = client
        .media()
        .get_media_content(&request, true) // use_cache=true
        .await
        .map_err(|e| anyhow::anyhow!("Failed to download media: {}", e))?;

    // Write to file
    let mut file = tokio::fs::File::create(&file_path).await?;
    file.write_all(&data).await?;

    tracing::info!(
        filename = %unique_filename,
        size = data.len(),
        "Downloaded attachment"
    );

    Ok(format!("attachments/{}", unique_filename))
}

/// Sanitize a filename to only contain safe characters
pub fn sanitize_filename(filename: &str) -> String {
    filename
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '.' || *c == '-' || *c == '_')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("test.txt"), "test.txt");
        assert_eq!(sanitize_filename("my file.pdf"), "myfile.pdf");
        // Dots are allowed (for extensions), so ../../../ becomes ......
        assert_eq!(sanitize_filename("../../../etc/passwd"), "......etcpasswd");
        assert_eq!(sanitize_filename("image (1).png"), "image1.png");
    }

    #[test]
    fn test_sanitize_filename_preserves_extension() {
        assert_eq!(sanitize_filename("report.pdf"), "report.pdf");
        assert_eq!(sanitize_filename("photo.jpg"), "photo.jpg");
        assert_eq!(sanitize_filename("data-2024.csv"), "data-2024.csv");
    }
}
