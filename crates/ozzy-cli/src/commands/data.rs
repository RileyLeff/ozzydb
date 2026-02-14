//! `ozzy data` — manage data atoms (upload, list, show, describe, yank, download).

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use super::shared;

// ── Response types ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct UploadResponse {
    name: String,
    hash: String,
    content_type: String,
    byte_size: i64,
    deduplicated: bool,
    collection_version: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct DataAtomListItem {
    name: String,
    hash: String,
    #[allow(dead_code)]
    content_type: String,
    byte_size: i64,
    yanked: bool,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct DataAtomDetail {
    name: String,
    hash: String,
    content_type: String,
    byte_size: i64,
    uploaded_by: String,
    yanked: bool,
    yank_reason: Option<String>,
    yanked_at: Option<String>,
    created_at: String,
    metadata: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct MetadataEntryResponse {
    field: String,
    value: serde_json::Value,
    set_by: String,
    created_at: String,
}

// ── Commands ────────────────────────────────────────────────────

/// Upload one or more files as data atoms.
pub async fn upload(
    files: &[String],
    name: Option<&str>,
    description: Option<&str>,
    tags: Option<&str>,
    collection: Option<&str>,
) -> Result<()> {
    if files.is_empty() {
        bail!("No files specified. Usage: ozzy data upload <file> [<file> ...]");
    }

    if files.len() > 1 && name.is_some() {
        bail!("--name can only be used with a single file");
    }

    let creds = shared::require_auth()?;
    let project = shared::load_project_from_toml()?;
    let registry_url = shared::registry_url(&creds);
    let client = shared::http_client()?;

    for file_path in files {
        let path = std::path::Path::new(file_path);
        if !path.exists() {
            bail!("File not found: {}", file_path);
        }

        let file_bytes = std::fs::read(path)
            .with_context(|| format!("Failed to read {}", file_path))?;

        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unnamed");

        // Build multipart form
        let file_part = reqwest::multipart::Part::bytes(file_bytes)
            .file_name(file_name.to_string());

        let mut form = reqwest::multipart::Form::new()
            .part("file", file_part)
            .text("project", project.project_path());

        if let Some(n) = name {
            form = form.text("name", n.to_string());
        }
        if let Some(d) = description {
            form = form.text("description", d.to_string());
        }
        if let Some(t) = tags {
            form = form.text("tags", t.to_string());
        }
        if let Some(c) = collection {
            form = form.text("collection", c.to_string());
        }

        eprintln!("Uploading {}...", file_path);

        let resp = client
            .post(format!("{}/api/v1/data/upload", registry_url))
            .bearer_auth(&creds.token)
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = shared::extract_error(resp).await;
            bail!("Upload failed for {}: {}", file_path, err);
        }

        let upload: UploadResponse = resp.json().await?;
        println!(
            "Uploaded: {} ({}, {} bytes, hash: {}{})",
            upload.name,
            upload.content_type,
            upload.byte_size,
            upload.hash.get(..12).unwrap_or(&upload.hash),
            if upload.deduplicated { ", deduplicated" } else { "" }
        );
        if let Some(ver) = upload.collection_version {
            println!("  Added to collection (version {})", ver);
        }
    }

    Ok(())
}

/// List data atoms in the current project.
pub async fn ls() -> Result<()> {
    let creds = shared::require_auth()?;
    let project = shared::load_project_from_toml()?;
    let registry_url = shared::registry_url(&creds);
    let client = shared::http_client()?;

    let resp = client
        .get(format!(
            "{}/api/v1/data/{}/{}",
            registry_url, project.owner, project.slug
        ))
        .bearer_auth(&creds.token)
        .send()
        .await?;

    if !resp.status().is_success() {
        let err = shared::extract_error(resp).await;
        bail!("Failed to list data: {}", err);
    }

    let atoms: Vec<DataAtomListItem> = resp.json().await?;

    if atoms.is_empty() {
        println!("No data atoms. Upload with `ozzy data upload <file>`.");
        return Ok(());
    }

    println!(
        "{:<24} {:<14} {:<10} {:<8} {}",
        "NAME", "HASH", "SIZE", "YANKED", "CREATED"
    );
    for atom in &atoms {
        println!(
            "{:<24} {:<14} {:<10} {:<8} {}",
            atom.name,
            atom.hash.get(..12).unwrap_or(&atom.hash),
            format_bytes(atom.byte_size),
            if atom.yanked { "yes" } else { "" },
            atom.created_at.get(..10).unwrap_or(&atom.created_at),
        );
    }

    Ok(())
}

/// Show data atom details.
pub async fn show(name: &str) -> Result<()> {
    shared::validate_name(name, "data atom")?;
    let creds = shared::require_auth()?;
    let project = shared::load_project_from_toml()?;
    let registry_url = shared::registry_url(&creds);
    let client = shared::http_client()?;

    let resp = client
        .get(format!(
            "{}/api/v1/data/{}/{}/{}",
            registry_url, project.owner, project.slug, name
        ))
        .bearer_auth(&creds.token)
        .send()
        .await?;

    if !resp.status().is_success() {
        let err = shared::extract_error(resp).await;
        bail!("Failed to show data atom '{}': {}", name, err);
    }

    let detail: DataAtomDetail = resp.json().await?;

    println!("Name:         {}", detail.name);
    println!("Hash:         {}", detail.hash);
    println!("Content-Type: {}", detail.content_type);
    println!("Size:         {} ({})", format_bytes(detail.byte_size), detail.byte_size);
    println!("Uploaded by:  {}", detail.uploaded_by);
    println!("Created:      {}", detail.created_at);

    if detail.yanked {
        println!("Status:       YANKED");
        if let Some(reason) = &detail.yank_reason {
            println!("Yank reason:  {}", reason);
        }
        if let Some(at) = &detail.yanked_at {
            println!("Yanked at:    {}", at);
        }
    }

    if !detail.metadata.is_empty() {
        println!("\nMetadata:");
        for (field, value) in &detail.metadata {
            println!("  {}: {}", field, value);
        }
    }

    Ok(())
}

/// Update metadata on a data atom.
pub async fn describe(
    name: &str,
    set_description: Option<&str>,
    history: bool,
) -> Result<()> {
    shared::validate_name(name, "data atom")?;
    let creds = shared::require_auth()?;
    let project = shared::load_project_from_toml()?;
    let registry_url = shared::registry_url(&creds);
    let client = shared::http_client()?;

    if history {
        // Show metadata history
        let resp = client
            .get(format!(
                "{}/api/v1/data/{}/{}/{}/metadata",
                registry_url, project.owner, project.slug, name
            ))
            .bearer_auth(&creds.token)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = shared::extract_error(resp).await;
            bail!("Failed to get metadata history: {}", err);
        }

        let entries: Vec<MetadataEntryResponse> = resp.json().await?;

        if entries.is_empty() {
            println!("No metadata history for '{}'.", name);
            return Ok(());
        }

        println!(
            "{:<16} {:<20} {:<16} {}",
            "FIELD", "VALUE", "SET BY", "DATE"
        );
        for entry in &entries {
            println!(
                "{:<16} {:<20} {:<16} {}",
                entry.field,
                entry.value.to_string().chars().take(18).collect::<String>(),
                entry.set_by,
                entry.created_at.get(..10).unwrap_or(&entry.created_at),
            );
        }
        return Ok(());
    }

    if let Some(desc) = set_description {
        let body = serde_json::json!({
            "field": "description",
            "value": desc,
        });

        let resp = client
            .post(format!(
                "{}/api/v1/data/{}/{}/{}/describe",
                registry_url, project.owner, project.slug, name
            ))
            .bearer_auth(&creds.token)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = shared::extract_error(resp).await;
            bail!("Failed to update description: {}", err);
        }

        println!("Updated description for '{}'.", name);
        return Ok(());
    }

    bail!("Specify --set-description or --history");
}

/// Yank a data atom.
pub async fn yank(name: &str, reason: &str) -> Result<()> {
    shared::validate_name(name, "data atom")?;
    let creds = shared::require_auth()?;
    let project = shared::load_project_from_toml()?;
    let registry_url = shared::registry_url(&creds);
    let client = shared::http_client()?;

    let body = serde_json::json!({ "reason": reason });

    let resp = client
        .post(format!(
            "{}/api/v1/data/{}/{}/{}/yank",
            registry_url, project.owner, project.slug, name
        ))
        .bearer_auth(&creds.token)
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let err = shared::extract_error(resp).await;
        bail!("Failed to yank '{}': {}", name, err);
    }

    println!("Yanked data atom '{}'.", name);
    Ok(())
}

/// Download a data atom.
pub async fn download(name: &str, output: Option<&str>) -> Result<()> {
    shared::validate_name(name, "data atom")?;
    let creds = shared::require_auth()?;
    let project = shared::load_project_from_toml()?;
    let registry_url = shared::registry_url(&creds);
    let client = shared::http_client_no_redirect()?;

    let resp = client
        .get(format!(
            "{}/api/v1/data/{}/{}/{}/download",
            registry_url, project.owner, project.slug, name
        ))
        .bearer_auth(&creds.token)
        .send()
        .await?;

    let status = resp.status();

    // Handle redirect to presigned URL
    if status == reqwest::StatusCode::FOUND || status == reqwest::StatusCode::TEMPORARY_REDIRECT {
        let location = resp
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| anyhow::anyhow!("Redirect missing Location header"))?
            .to_string();
        let expected_hash = resp
            .headers()
            .get("x-ozzydb-content-hash")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        // Follow redirect without auth (presigned URL)
        let dl_client = shared::http_client()?;
        let dl_resp = dl_client.get(&location).send().await?;

        if !dl_resp.status().is_success() {
            bail!("Download failed ({})", dl_resp.status());
        }

        let bytes = dl_resp.bytes().await?;
        verify_download_hash(&bytes, expected_hash.as_deref())?;

        let out_path = output.unwrap_or(name);
        std::fs::write(out_path, &bytes)
            .with_context(|| format!("Failed to write {}", out_path))?;
        println!("Downloaded {} ({} bytes) to {}", name, bytes.len(), out_path);
        return Ok(());
    }

    if !status.is_success() {
        let err = shared::extract_error(resp).await;
        bail!("Download failed for '{}': {}", name, err);
    }

    // Direct response (local storage mode) — no hash header available
    let bytes = resp.bytes().await?;
    let out_path = output.unwrap_or(name);
    std::fs::write(out_path, &bytes)
        .with_context(|| format!("Failed to write {}", out_path))?;
    println!("Downloaded {} ({} bytes) to {}", name, bytes.len(), out_path);

    Ok(())
}

/// Verify downloaded bytes against expected BLAKE3 hash (if provided).
fn verify_download_hash(bytes: &[u8], expected: Option<&str>) -> Result<()> {
    if let Some(expected) = expected {
        let actual = blake3::hash(bytes).to_hex().to_string();
        if actual != expected {
            bail!(
                "Hash mismatch: expected {}, got {}. Download may be corrupted.",
                expected,
                actual
            );
        }
    }
    Ok(())
}

/// Format byte size for display.
fn format_bytes(bytes: i64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(1048576), "1.0 MB");
        assert_eq!(format_bytes(1073741824), "1.0 GB");
    }

    #[test]
    fn test_upload_response_deserialization() {
        let json = serde_json::json!({
            "name": "readings",
            "hash": "abc123",
            "content_type": "text/csv",
            "byte_size": 1024,
            "deduplicated": false,
            "collection_version": null,
        });
        let resp: UploadResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.name, "readings");
        assert!(!resp.deduplicated);
        assert!(resp.collection_version.is_none());
    }

    #[test]
    fn test_upload_response_with_collection() {
        let json = serde_json::json!({
            "name": "readings",
            "hash": "abc123",
            "content_type": "text/csv",
            "byte_size": 1024,
            "deduplicated": true,
            "collection_version": 3,
        });
        let resp: UploadResponse = serde_json::from_value(json).unwrap();
        assert!(resp.deduplicated);
        assert_eq!(resp.collection_version, Some(3));
    }

    #[test]
    fn test_data_atom_list_deserialization() {
        let json = serde_json::json!([{
            "name": "test",
            "hash": "abc",
            "content_type": "text/csv",
            "byte_size": 100,
            "yanked": false,
            "created_at": "2025-01-01T00:00:00Z",
        }]);
        let atoms: Vec<DataAtomListItem> = serde_json::from_value(json).unwrap();
        assert_eq!(atoms.len(), 1);
        assert_eq!(atoms[0].name, "test");
    }
}
