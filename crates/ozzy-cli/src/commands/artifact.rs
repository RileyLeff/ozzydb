//! `ozzy artifact` — manage first-class v4 artifacts.

use anyhow::{Context, Result, anyhow, bail};
use ozzy_core::artifacts::{ArtifactManifest, ArtifactManifestEntry};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::shared;

#[derive(Debug, Deserialize)]
struct UploadResponse {
    artifact_id: Uuid,
    content_hash: String,
    content_type: String,
    byte_size: i64,
    deduplicated: bool,
}

#[derive(Debug, Deserialize)]
struct ArtifactSummary {
    id: Uuid,
    artifact_kind: String,
    content_hash: Option<String>,
    source_invocation_id: Option<Uuid>,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct ArtifactDetail {
    id: Uuid,
    artifact_kind: String,
    content_hash: Option<String>,
    manifest: Option<ArtifactManifest>,
    source_invocation_id: Option<Uuid>,
    created_at: String,
}

#[derive(Debug, Serialize)]
struct ConformanceRequest<'a> {
    #[serde(rename = "type")]
    type_ref: &'a str,
    verify: bool,
}

#[derive(Debug, Deserialize)]
struct ArtifactConformanceResponse {
    artifact_id: Uuid,
    records: Vec<ConformanceRecordDetail>,
}

#[derive(Debug, Deserialize)]
struct ConformanceRecordDetail {
    id: Uuid,
    status: String,
    type_version: TypeVersionDetail,
    created_at: String,
    updated_at: String,
    attempts: Vec<VerificationAttemptDetail>,
}

#[derive(Debug, Deserialize)]
struct VerificationAttemptDetail {
    id: Uuid,
    verifier: String,
    attempt_kind: String,
    verdict: Option<String>,
    diagnostics: serde_json::Value,
    evidence: Option<serde_json::Value>,
    failure_error: Option<String>,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct TypeVersionDetail {
    id: Uuid,
    name: String,
    version: String,
    canonical_type_key: String,
    expr: serde_json::Value,
}

pub async fn upload(files: &[String], content_type: Option<&str>) -> Result<()> {
    if files.is_empty() {
        bail!("No files specified. Usage: ozzy artifact upload <file> [<file> ...]");
    }

    let creds = shared::require_auth()?;
    let project = shared::load_project_from_toml()?;
    let client = shared::http_client()?;
    let base_url = format!(
        "{}/api/v1/artifacts/{}/{}",
        shared::registry_url(&creds),
        project.owner,
        project.slug
    );

    for file_path in files {
        let path = std::path::Path::new(file_path);
        if !path.exists() {
            bail!("File not found: {}", file_path);
        }

        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| anyhow!("Invalid filename: {}", file_path))?;
        let file_bytes =
            std::fs::read(path).with_context(|| format!("Failed to read file '{}'", file_path))?;

        let mut form = reqwest::multipart::Form::new().part(
            "file",
            reqwest::multipart::Part::bytes(file_bytes).file_name(file_name.to_string()),
        );
        if let Some(content_type) = content_type {
            form = form.text("content_type", content_type.to_string());
        }

        let resp = client
            .post(format!("{}/upload", base_url))
            .bearer_auth(&creds.token)
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = shared::extract_error(resp).await;
            bail!("Upload failed for '{}': {}", file_path, err);
        }

        let upload: UploadResponse = resp.json().await?;
        println!(
            "Uploaded {} -> {} ({}, {} bytes{})",
            file_path,
            upload.artifact_id,
            upload.content_type,
            upload.byte_size,
            if upload.deduplicated {
                ", deduplicated"
            } else {
                ""
            }
        );
        println!("  hash: {}", upload.content_hash);
    }

    Ok(())
}

pub async fn ls() -> Result<()> {
    let creds = shared::require_auth()?;
    let project = shared::load_project_from_toml()?;
    let client = shared::http_client()?;

    let resp = client
        .get(format!(
            "{}/api/v1/artifacts/{}/{}",
            shared::registry_url(&creds),
            project.owner,
            project.slug
        ))
        .bearer_auth(&creds.token)
        .send()
        .await?;

    if !resp.status().is_success() {
        let err = shared::extract_error(resp).await;
        bail!("Failed to list artifacts: {}", err);
    }

    let artifacts: Vec<ArtifactSummary> = resp.json().await?;
    if artifacts.is_empty() {
        println!("No artifacts in this project.");
        return Ok(());
    }

    println!(
        "{:<36} {:<10} {:<14} {:<36} {}",
        "ARTIFACT_ID", "KIND", "HASH", "SOURCE_INVOCATION", "CREATED"
    );
    for artifact in artifacts {
        println!(
            "{:<36} {:<10} {:<14} {:<36} {}",
            artifact.id,
            artifact.artifact_kind,
            artifact
                .content_hash
                .as_deref()
                .map(|hash| &hash[..std::cmp::min(12, hash.len())])
                .unwrap_or(""),
            artifact
                .source_invocation_id
                .map(|id| id.to_string())
                .unwrap_or_default(),
            artifact.created_at,
        );
    }

    Ok(())
}

pub async fn show(artifact_id: &str) -> Result<()> {
    let artifact_id = parse_uuid(artifact_id, "artifact_id")?;
    let creds = shared::require_auth()?;
    let project = shared::load_project_from_toml()?;
    let client = shared::http_client()?;

    let base_url = format!(
        "{}/api/v1/artifacts/{}/{}/{}",
        shared::registry_url(&creds),
        project.owner,
        project.slug,
        artifact_id
    );

    let resp = client
        .get(&base_url)
        .bearer_auth(&creds.token)
        .send()
        .await?;
    if !resp.status().is_success() {
        let err = shared::extract_error(resp).await;
        bail!("Failed to get artifact '{}': {}", artifact_id, err);
    }
    let artifact: ArtifactDetail = resp.json().await?;

    println!("Artifact:    {}", artifact.id);
    println!("Kind:        {}", artifact.artifact_kind);
    if let Some(hash) = artifact.content_hash.as_deref() {
        println!("Content hash: {}", hash);
    }
    if let Some(source_invocation_id) = artifact.source_invocation_id {
        println!("Invocation:   {}", source_invocation_id);
    }
    println!("Created:     {}", artifact.created_at);

    if let Some(manifest) = artifact.manifest {
        println!("\nManifest:");
        print_manifest(&manifest);
    }

    let conformance_resp = client
        .get(format!("{}/conformance", base_url))
        .bearer_auth(&creds.token)
        .send()
        .await?;

    if !conformance_resp.status().is_success() {
        let err = shared::extract_error(conformance_resp).await;
        bail!(
            "Failed to get artifact conformance '{}': {}",
            artifact_id,
            err
        );
    }

    let conformance: ArtifactConformanceResponse = conformance_resp.json().await?;
    println!("\nConformance for artifact {}:", conformance.artifact_id);
    if conformance.records.is_empty() {
        println!("  none");
    } else {
        for record in conformance.records {
            println!(
                "  {}  {}  {}@{}  {}",
                record.id,
                record.status,
                record.type_version.name,
                record.type_version.version,
                record.updated_at,
            );
            println!(
                "    type_version_id={} canonical_type_key={}",
                record.type_version.id, record.type_version.canonical_type_key
            );
            println!(
                "    declared_at={} expr={}",
                record.created_at,
                serde_json::to_string_pretty(&record.type_version.expr)?
            );
            for attempt in record.attempts {
                println!(
                    "    attempt {} {} {} {}",
                    attempt.id,
                    attempt.verifier,
                    attempt.attempt_kind,
                    attempt.verdict.as_deref().unwrap_or("pending")
                );
                println!("      created_at={}", attempt.created_at);
                if let Some(error) = attempt.failure_error {
                    println!("      failure_error={}", error);
                }
                println!(
                    "      diagnostics={}",
                    serde_json::to_string_pretty(&attempt.diagnostics)?
                );
                if let Some(evidence) = attempt.evidence {
                    println!(
                        "      evidence={}",
                        serde_json::to_string_pretty(&evidence)?
                    );
                }
            }
        }
    }

    Ok(())
}

pub async fn download(artifact_id: &str, output: Option<&str>) -> Result<()> {
    let artifact_id = parse_uuid(artifact_id, "artifact_id")?;
    let creds = shared::require_auth()?;
    let project = shared::load_project_from_toml()?;
    let client = shared::http_client_no_redirect()?;

    let resp = client
        .get(format!(
            "{}/api/v1/artifacts/{}/{}/{}/download",
            shared::registry_url(&creds),
            project.owner,
            project.slug,
            artifact_id
        ))
        .bearer_auth(&creds.token)
        .send()
        .await?;

    if !resp.status().is_success() {
        let err = shared::extract_error(resp).await;
        bail!("Failed to download artifact '{}': {}", artifact_id, err);
    }

    let location = resp
        .headers()
        .get("location")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| anyhow!("Download response missing Location header"))?
        .to_string();
    let content_type = resp
        .headers()
        .get("x-ozzydb-content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream");

    let bytes = reqwest::Client::new()
        .get(&location)
        .send()
        .await
        .context("Failed to follow artifact download redirect")?
        .bytes()
        .await
        .context("Failed to download artifact bytes")?;

    if let Some(path) = output {
        std::fs::write(path, &bytes).with_context(|| format!("Failed to write '{}'", path))?;
        println!("Wrote {} bytes to {}", bytes.len(), path);
    } else {
        let ext = extension_for_content_type(content_type);
        let filename = format!("artifact-{}.{}", artifact_id, ext);
        std::fs::write(&filename, &bytes)
            .with_context(|| format!("Failed to write '{}'", filename))?;
        println!("Wrote {} bytes to {}", bytes.len(), filename);
    }

    Ok(())
}

pub async fn bundle(entries: &[String]) -> Result<()> {
    if entries.is_empty() {
        bail!("No entries specified. Usage: ozzy artifact bundle --entry name=artifact_uuid [...]");
    }

    let mut parsed = std::collections::BTreeMap::new();
    for entry in entries {
        let (name, artifact_id) = entry.split_once('=').ok_or_else(|| {
            anyhow!(
                "Invalid bundle entry '{}'. Expected name=artifact_uuid",
                entry
            )
        })?;
        shared::validate_name(name, "bundle entry")?;
        let artifact_id = parse_uuid(artifact_id, "bundle entry artifact_id")?;
        if parsed
            .insert(name.to_string(), ArtifactManifestEntry { artifact_id })
            .is_some()
        {
            bail!("Duplicate bundle entry name '{}'", name);
        }
    }

    create_manifest(&ArtifactManifest::Bundle { entries: parsed }).await
}

pub async fn collection(items: &[String]) -> Result<()> {
    if items.is_empty() {
        bail!("No items specified. Usage: ozzy artifact collection <artifact_uuid> [...]");
    }

    let items = items
        .iter()
        .map(|value| {
            parse_uuid(value, "collection item")
                .map(|artifact_id| ArtifactManifestEntry { artifact_id })
        })
        .collect::<Result<Vec<_>>>()?;

    create_manifest(&ArtifactManifest::Collection { items }).await
}

pub async fn conformance(artifact_id: &str, type_ref: &str, verify: bool) -> Result<()> {
    let artifact_id = parse_uuid(artifact_id, "artifact_id")?;
    let creds = shared::require_auth()?;
    let project = shared::load_project_from_toml()?;
    let client = shared::http_client()?;

    let resp = client
        .post(format!(
            "{}/api/v1/artifacts/{}/{}/{}/conformance",
            shared::registry_url(&creds),
            project.owner,
            project.slug,
            artifact_id
        ))
        .bearer_auth(&creds.token)
        .json(&ConformanceRequest { type_ref, verify })
        .send()
        .await?;

    if !resp.status().is_success() {
        let err = shared::extract_error(resp).await;
        bail!(
            "Failed to declare conformance for artifact '{}': {}",
            artifact_id,
            err
        );
    }

    let record: ConformanceRecordDetail = resp.json().await?;
    println!(
        "Conformance {}: {} {}@{}",
        record.id, record.status, record.type_version.name, record.type_version.version
    );
    Ok(())
}

async fn create_manifest(manifest: &ArtifactManifest) -> Result<()> {
    manifest.validate()?;
    let creds = shared::require_auth()?;
    let project = shared::load_project_from_toml()?;
    let client = shared::http_client()?;

    let resp = client
        .post(format!(
            "{}/api/v1/artifacts/{}/{}/manifest",
            shared::registry_url(&creds),
            project.owner,
            project.slug
        ))
        .bearer_auth(&creds.token)
        .json(manifest)
        .send()
        .await?;

    if !resp.status().is_success() {
        let err = shared::extract_error(resp).await;
        bail!("Failed to create manifest artifact: {}", err);
    }

    let artifact: ArtifactDetail = resp.json().await?;
    println!("Created manifest artifact {}", artifact.id);
    Ok(())
}

fn print_manifest(manifest: &ArtifactManifest) {
    match manifest {
        ArtifactManifest::Bundle { entries } => {
            println!("  kind: bundle");
            for (name, entry) in entries {
                println!("  {} = {}", name, entry.artifact_id);
            }
        }
        ArtifactManifest::Collection { items } => {
            println!("  kind: collection");
            for (idx, entry) in items.iter().enumerate() {
                println!("  [{}] {}", idx, entry.artifact_id);
            }
        }
    }
}

fn parse_uuid(value: &str, field: &str) -> Result<Uuid> {
    Uuid::parse_str(value).with_context(|| format!("Invalid {} '{}': expected UUID", field, value))
}

fn extension_for_content_type(content_type: &str) -> &str {
    match content_type {
        "application/vnd.apache.parquet" => "parquet",
        "text/csv" => "csv",
        "application/json" => "json",
        "application/octet-stream" => "bin",
        _ => "bin",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_uuid_rejects_invalid_values() {
        let err = parse_uuid("not-a-uuid", "artifact_id")
            .unwrap_err()
            .to_string();
        assert!(err.contains("Invalid artifact_id"));
    }
}
