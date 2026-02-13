//! `ozzy fetch` — fetch and execute a remote endpoint.
//!
//! Calls the registry's fetch API and streams the result to stdout or a file.
//! The server handles everything: data resolution, environment, execution, caching.

use anyhow::{Context, Result, bail};

use super::auth::load_credentials;

/// Execute `ozzy fetch <owner/project/endpoint[@ref]>`.
pub async fn run(endpoint: &str, output: Option<&str>, params: &[String]) -> Result<()> {
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

    // Load credentials (optional — public projects don't require auth)
    let creds = load_credentials()?;

    // Determine registry URL (from credentials or default)
    let registry_url = creds
        .as_ref()
        .map(|c| c.registry_url.as_str())
        .unwrap_or("https://api.ozzydb.com");

    // Build request
    let client = reqwest::Client::new();
    let url = format!(
        "{}/api/v1/fetch/{}/{}/{}",
        registry_url, owner, project, ep_name,
    );

    let mut query: Vec<(&str, String)> = Vec::new();
    let ref_string;
    if let Some(r) = git_ref {
        ref_string = r.to_string();
        query.push(("ref", ref_string.clone()));
    }
    for param in params {
        if let Some((key, value)) = param.split_once('=') {
            query.push((key, value.to_string()));
        } else {
            bail!("Invalid param format '{}'. Expected key=value", param);
        }
    }

    let mut request = client.get(&url).query(&query);
    if let Some(ref creds) = creds {
        request = request.header("Authorization", format!("Bearer {}", creds.token));
    }

    let response = request
        .send()
        .await
        .context("Failed to connect to registry")?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        bail!("Fetch failed ({}): {}", status, body);
    }

    // Extract metadata headers
    let hash = response
        .headers()
        .get("x-ozzydb-hash")
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    let verification = response
        .headers()
        .get("x-ozzydb-verification")
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    let cache = response
        .headers()
        .get("x-ozzydb-cache")
        .and_then(|v| v.to_str().ok())
        .map(String::from);

    // Read body
    let bytes = response
        .bytes()
        .await
        .context("Failed to read response body")?;

    // Write output
    if let Some(path) = output {
        std::fs::write(path, &bytes).context("Failed to write output file")?;
        eprintln!("Wrote {} bytes to {}", bytes.len(), path);
    } else {
        use std::io::Write;
        std::io::stdout().write_all(&bytes)?;
    }

    // Print metadata to stderr
    if let Some(h) = hash {
        eprintln!("Hash: {}", h);
    }
    if let Some(v) = verification {
        eprintln!("Verification: {}", v);
    }
    if let Some(c) = cache {
        eprintln!("Cache: {}", c);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
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
}
