//! `ozzy fetch` — fetch and execute a remote endpoint.
//!
//! POSTs to the registry's fetch API, polls for job completion, then
//! downloads the result to stdout or a file.

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use super::auth::load_credentials;
use super::shared;

/// Fetch response from POST /api/v1/fetch/{owner}/{project}/{endpoint}.
#[derive(Debug, Deserialize)]
struct FetchResponse {
    job_id: String,
    status: String,
    output_url: Option<String>,
    output_hash: Option<String>,
}

/// Job status response from GET /api/v1/jobs/{id}.
#[derive(Debug, Deserialize)]
struct JobStatus {
    status: String,
    node_status: serde_json::Value,
    output_hash: Option<String>,
    error_message: Option<String>,
}

/// Execute `ozzy fetch <owner/project/endpoint[@ref]>`.
pub async fn run(
    endpoint: &str,
    output: Option<&str>,
    params: &[String],
    timeout_secs: u64,
) -> Result<()> {
    // Parse reference: owner/project/endpoint[@ref]
    let (path, git_ref) = match endpoint.split_once('@') {
        Some((p, r)) => (p, Some(r)),
        None => (endpoint, None),
    };

    let parts: Vec<&str> = path.splitn(3, '/').collect();
    if parts.len() != 3 {
        bail!(
            "Invalid endpoint reference '{}'. Expected: owner/project/endpoint[@ref]",
            endpoint
        );
    }
    let (owner, project, ep_name) = (parts[0], parts[1], parts[2]);
    shared::validate_name(owner, "owner")?;
    shared::validate_name(project, "project")?;
    shared::validate_name(ep_name, "endpoint")?;

    if let Some(r) = git_ref {
        if r.is_empty() {
            bail!("Empty ref in '{}'. Omit '@' or provide a ref name.", endpoint);
        }
    }

    // Load credentials (optional — public projects don't require auth)
    let creds = load_credentials()?;

    // Extract token and registry URL upfront to avoid borrow/move issues
    let token = creds.as_ref().map(|c| c.token.clone());
    let registry_url = creds
        .as_ref()
        .map(|c| c.registry_url.clone())
        .unwrap_or_else(|| "https://api.ozzydb.com".to_string());

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .context("Failed to create HTTP client")?;

    // Build query params: ref + user params
    let mut query: Vec<(&str, String)> = Vec::new();
    if let Some(r) = git_ref {
        query.push(("ref", r.to_string()));
    }
    for param in params {
        if let Some((key, value)) = param.split_once('=') {
            query.push((key, value.to_string()));
        } else {
            bail!("Invalid param format '{}'. Expected key=value", param);
        }
    }

    // POST to fetch endpoint
    let fetch_url = format!(
        "{}/api/v1/fetch/{}/{}/{}",
        registry_url, owner, project, ep_name,
    );

    let mut request = client.post(&fetch_url).query(&query);
    if let Some(t) = &token {
        request = request.header("Authorization", format!("Bearer {}", t));
    }

    let response = request
        .send()
        .await
        .context("Failed to connect to registry")?;

    let status_code = response.status();
    if !status_code.is_success() {
        let body = response.text().await.unwrap_or_default();
        bail!("Fetch failed ({}): {}", status_code, body);
    }

    let fetch_resp: FetchResponse = response
        .json()
        .await
        .context("Failed to parse fetch response")?;

    // If already done (cache hit), download output immediately
    if fetch_resp.status == "done" {
        let output_url = fetch_resp
            .output_url
            .ok_or_else(|| anyhow::anyhow!("Server returned done status but no output URL"))?;
        eprintln!("Cache hit");
        download_output(
            &client,
            &registry_url,
            &output_url,
            token.as_deref(),
            output,
            fetch_resp.output_hash.as_deref(),
        )
        .await?;
        return Ok(());
    }

    // Poll for job completion
    let job_id = &fetch_resp.job_id;
    let poll_url = format!("{}/api/v1/jobs/{}", registry_url, job_id);
    let poll_interval = std::time::Duration::from_secs(2);
    let timeout = std::time::Duration::from_secs(timeout_secs);
    let poll_start = std::time::Instant::now();
    let mut last_node_status = String::new();

    loop {
        tokio::time::sleep(poll_interval).await;

        if poll_start.elapsed() > timeout {
            bail!("Job {} did not complete within {}s", job_id, timeout_secs);
        }

        let mut request = client.get(&poll_url);
        if let Some(t) = &token {
            request = request.header("Authorization", format!("Bearer {}", t));
        }

        let response = request.send().await.context("Failed to poll job status")?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            bail!("Job status check failed: {}", body);
        }

        let job: JobStatus = response
            .json()
            .await
            .context("Failed to parse job status")?;

        // Display per-node progress
        let node_display = format_node_status(&job.node_status);
        if node_display != last_node_status {
            eprint!("\r\x1b[K{}", node_display);
            last_node_status = node_display;
        }

        match job.status.as_str() {
            "done" => {
                eprintln!("\r\x1b[KDone");
                let output_url = format!("/api/v1/jobs/{}/output", job_id);
                download_output(
                    &client,
                    &registry_url,
                    &output_url,
                    token.as_deref(),
                    output,
                    job.output_hash.as_deref(),
                )
                .await?;
                return Ok(());
            }
            "failed" => {
                eprintln!();
                let msg = job
                    .error_message
                    .unwrap_or_else(|| "unknown error".to_string());
                bail!("Job failed: {}", msg);
            }
            _ => {} // queued or running — keep polling
        }
    }
}

/// Format node statuses for display.
fn format_node_status(node_status: &serde_json::Value) -> String {
    let Some(obj) = node_status.as_object() else {
        return String::new();
    };

    let mut parts: Vec<String> = Vec::new();
    for (name, status) in obj {
        let symbol = match status.as_str().unwrap_or("") {
            "done" => "+",
            "running" => "~",
            _ => ".",
        };
        parts.push(format!("[{}]{}", symbol, name));
    }
    parts.sort();
    parts.join(" ")
}

/// Download output from the job output endpoint.
async fn download_output(
    client: &reqwest::Client,
    registry_url: &str,
    output_path: &str,
    token: Option<&str>,
    file_output: Option<&str>,
    hash: Option<&str>,
) -> Result<()> {
    let url = if output_path.starts_with("http://") || output_path.starts_with("https://") {
        output_path.to_string()
    } else {
        format!("{}{}", registry_url, output_path)
    };

    let mut request = client.get(&url);
    if let Some(t) = token {
        request = request.header("Authorization", format!("Bearer {}", t));
    }

    let response = request.send().await.context("Failed to download output")?;
    let status = response.status();

    // Handle redirect (presigned URL)
    if status == reqwest::StatusCode::FOUND || status == reqwest::StatusCode::TEMPORARY_REDIRECT {
        let location = response
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| anyhow::anyhow!("Redirect response missing Location header"))?;

        // Follow redirect (no auth header, separate client with redirect policy)
        let dl_client = reqwest::Client::new();
        let bytes = dl_client
            .get(location)
            .send()
            .await
            .context("Failed to follow presigned URL redirect")?
            .bytes()
            .await
            .context("Failed to read output from presigned URL")?;

        write_output(&bytes, file_output, hash)?;
        return Ok(());
    }

    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        bail!("Output download failed ({}): {}", status, body);
    }

    // Direct response (local storage mode)
    let bytes = response.bytes().await.context("Failed to read output")?;

    write_output(&bytes, file_output, hash)?;
    Ok(())
}

/// Write output bytes to file or stdout, verifying BLAKE3 hash if provided.
fn write_output(bytes: &[u8], file_output: Option<&str>, hash: Option<&str>) -> Result<()> {
    if let Some(expected) = hash {
        let actual = blake3::hash(bytes).to_hex().to_string();
        if actual != expected {
            bail!(
                "Hash mismatch: expected {}, got {}. Output may be corrupted.",
                expected,
                actual
            );
        }
    }

    if let Some(path) = file_output {
        std::fs::write(path, bytes).context("Failed to write output file")?;
        eprintln!("Wrote {} bytes to {}", bytes.len(), path);
    } else {
        use std::io::Write;
        std::io::stdout().write_all(bytes)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_remote_ref_with_ref() {
        let ep = "rileyleff/sapflux/corrected@v1.0";
        let (path, git_ref) = ep.split_once('@').unwrap();
        let parts: Vec<&str> = path.splitn(3, '/').collect();
        assert_eq!(parts, vec!["rileyleff", "sapflux", "corrected"]);
        assert_eq!(git_ref, "v1.0");
    }

    #[test]
    fn test_parse_remote_ref_without_ref() {
        let ep = "rileyleff/sapflux/corrected";
        assert!(ep.split_once('@').is_none());
        let parts: Vec<&str> = ep.splitn(3, '/').collect();
        assert_eq!(parts, vec!["rileyleff", "sapflux", "corrected"]);
    }

    #[test]
    fn test_parse_remote_ref_invalid() {
        let ep = "just-a-name";
        let parts: Vec<&str> = ep.splitn(3, '/').collect();
        assert_eq!(parts.len(), 1); // not enough parts
    }

    #[test]
    fn test_format_node_status_empty() {
        let status = serde_json::json!({});
        assert_eq!(format_node_status(&status), "");
    }

    #[test]
    fn test_format_node_status_mixed() {
        let status = serde_json::json!({
            "step1": "done",
            "step2": "running",
            "step3": "queued"
        });
        let formatted = format_node_status(&status);
        assert!(formatted.contains("[+]step1"));
        assert!(formatted.contains("[~]step2"));
        assert!(formatted.contains("[.]step3"));
    }

    #[test]
    fn test_format_node_status_all_done() {
        let status = serde_json::json!({
            "a": "done",
            "b": "done"
        });
        let formatted = format_node_status(&status);
        assert_eq!(formatted, "[+]a [+]b");
    }
}
