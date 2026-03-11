//! `ozzy fetch` — fetch and execute a remote endpoint.
//!
//! POSTs to the registry's v4 fetch API, polls for job completion, then
//! downloads the result to stdout or a file.

use std::collections::BTreeMap;

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use super::auth::load_credentials;
use super::shared;

#[derive(Debug, Serialize)]
struct FetchRequest {
    #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
    ref_name: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    params: BTreeMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    inputs: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct FetchResponse {
    job_id: String,
    status: String,
    output_url: Option<String>,
    output_hash: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JobStatus {
    status: String,
    node_status: serde_json::Value,
    output_hash: Option<String>,
    error_message: Option<String>,
}

pub async fn run(
    endpoint: &str,
    output: Option<&str>,
    params: &[String],
    inputs: &[String],
    timeout_secs: u64,
) -> Result<()> {
    let (path, git_ref) = match endpoint.split_once('@') {
        Some((path, git_ref)) => (path, Some(git_ref)),
        None => (endpoint, None),
    };

    let parts: Vec<&str> = path.splitn(3, '/').collect();
    if parts.len() != 3 {
        bail!(
            "Invalid endpoint reference '{}'. Expected: owner/project/endpoint[@ref]",
            endpoint
        );
    }
    let (owner, project, endpoint_name) = (parts[0], parts[1], parts[2]);
    shared::validate_name(owner, "owner")?;
    shared::validate_name(project, "project")?;
    shared::validate_name(endpoint_name, "endpoint")?;

    let request_body = FetchRequest {
        ref_name: git_ref.map(ToString::to_string),
        params: parse_params(params)?,
        inputs: parse_inputs(inputs)?,
    };

    let creds = load_credentials()?;
    let token = creds.as_ref().map(|creds| creds.token.clone());
    let registry_url = creds
        .as_ref()
        .map(|creds| creds.registry_url.clone())
        .unwrap_or_else(|| "https://api.ozzydb.com".to_string());

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .context("Failed to create HTTP client")?;

    let fetch_url = format!(
        "{}/api/v1/fetch/{}/{}/{}",
        registry_url, owner, project, endpoint_name
    );
    let mut request = client.post(&fetch_url).json(&request_body);
    if let Some(token) = &token {
        request = request.header("Authorization", format!("Bearer {}", token));
    }

    let response = request
        .send()
        .await
        .context("Failed to connect to registry")?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        bail!("Fetch failed ({}): {}", status, body);
    }

    let fetch_resp: FetchResponse = response
        .json()
        .await
        .context("Failed to parse fetch response")?;

    if fetch_resp.status == "done" {
        let output_url = fetch_resp
            .output_url
            .ok_or_else(|| anyhow!("Server returned done status but no output URL"))?;
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

    let poll_url = format!("{}/api/v1/jobs/{}", registry_url, fetch_resp.job_id);
    let poll_interval = std::time::Duration::from_secs(2);
    let timeout = std::time::Duration::from_secs(timeout_secs);
    let poll_start = std::time::Instant::now();
    let mut last_node_status = String::new();

    loop {
        tokio::time::sleep(poll_interval).await;
        if poll_start.elapsed() > timeout {
            bail!(
                "Job {} did not complete within {}s",
                fetch_resp.job_id,
                timeout_secs
            );
        }

        let mut request = client.get(&poll_url);
        if let Some(token) = &token {
            request = request.header("Authorization", format!("Bearer {}", token));
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
        let node_display = format_node_status(&job.node_status);
        if node_display != last_node_status {
            eprint!("\r\x1b[K{}", node_display);
            last_node_status = node_display;
        }

        match job.status.as_str() {
            "done" => {
                eprintln!("\r\x1b[KDone");
                let output_url = format!("/api/v1/jobs/{}/output", fetch_resp.job_id);
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
                bail!(
                    "Job failed: {}",
                    job.error_message
                        .unwrap_or_else(|| "unknown error".to_string())
                );
            }
            _ => {}
        }
    }
}

fn parse_params(params: &[String]) -> Result<BTreeMap<String, serde_json::Value>> {
    let mut out = BTreeMap::new();
    for raw in params {
        let (key, value) = raw
            .split_once('=')
            .ok_or_else(|| anyhow!("Invalid param '{}'. Expected key=JSON_VALUE", raw))?;
        if key.is_empty() {
            bail!("Invalid param '{}': empty key", raw);
        }
        let parsed = serde_json::from_str::<serde_json::Value>(value).with_context(|| {
            format!(
                "Invalid JSON for param '{}'. Strings must be quoted, for example name=\"oak\"",
                key
            )
        })?;
        if out.insert(key.to_string(), parsed).is_some() {
            bail!("Duplicate param '{}'", key);
        }
    }
    Ok(out)
}

fn parse_inputs(inputs: &[String]) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for raw in inputs {
        let (key, value) = raw
            .split_once('=')
            .ok_or_else(|| anyhow!("Invalid input '{}'. Expected name=artifact_uuid", raw))?;
        if key.is_empty() {
            bail!("Invalid input '{}': empty key", raw);
        }
        let uuid = uuid::Uuid::parse_str(value)
            .with_context(|| format!("Invalid artifact UUID for input '{}': {}", key, value))?;
        if out.insert(key.to_string(), uuid.to_string()).is_some() {
            bail!("Duplicate input binding '{}'", key);
        }
    }
    Ok(out)
}

fn format_node_status(node_status: &serde_json::Value) -> String {
    let Some(obj) = node_status.as_object() else {
        return String::new();
    };

    let mut parts = Vec::new();
    for (name, status) in obj {
        let symbol = match status.as_str().unwrap_or("") {
            "done" => "+",
            "running" => "~",
            "failed" => "!",
            _ => ".",
        };
        parts.push(format!("[{}]{}", symbol, name));
    }
    parts.sort();
    parts.join(" ")
}

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
    if let Some(token) = token {
        request = request.header("Authorization", format!("Bearer {}", token));
    }

    let response = request.send().await.context("Failed to download output")?;
    let status = response.status();

    if status == reqwest::StatusCode::FOUND || status == reqwest::StatusCode::TEMPORARY_REDIRECT {
        let location = response
            .headers()
            .get("location")
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| anyhow!("Redirect response missing Location header"))?;

        let bytes = reqwest::Client::new()
            .get(location)
            .send()
            .await
            .context("Failed to follow presigned URL redirect")?
            .bytes()
            .await
            .context("Failed to download bytes from presigned URL")?;

        if let Some(path) = file_output {
            std::fs::write(path, &bytes)
                .with_context(|| format!("Failed to write output file '{}'", path))?;
            eprintln!("Wrote {} bytes to {}", bytes.len(), path);
        } else {
            use std::io::Write;
            let mut stdout = std::io::stdout();
            stdout.write_all(&bytes)?;
            stdout.flush()?;
        }

        if let Some(hash) = hash {
            let actual_hash = blake3::hash(&bytes).to_hex().to_string();
            if actual_hash != hash {
                bail!(
                    "Output hash mismatch: expected {}, got {}",
                    hash,
                    actual_hash
                );
            }
        }

        return Ok(());
    }

    let body = response.text().await.unwrap_or_default();
    bail!("Failed to download output ({}): {}", status, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_params_requires_valid_json() {
        let err = parse_params(&["species=oak".to_string()])
            .unwrap_err()
            .to_string();
        assert!(err.contains("Strings must be quoted"));
    }

    #[test]
    fn parse_inputs_rejects_invalid_uuid() {
        let err = parse_inputs(&["raw=not-a-uuid".to_string()])
            .unwrap_err()
            .to_string();
        assert!(err.contains("Invalid artifact UUID"));
    }
}
