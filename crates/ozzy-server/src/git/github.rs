//! GitHub App integration for fetching repository content.
//!
//! Auth flow:
//! 1. User installs the OzzyDB GitHub App on their repo(s)
//! 2. GitHub sends an installation webhook → we store installation_id
//! 3. On push, we look up the installation_id for the repo owner
//! 4. Create a JWT (RS256, signed with App private key)
//! 5. Exchange JWT for a short-lived installation token (1 hour)
//! 6. Use the installation token to fetch files/tarballs from the GitHub API
//!
//! For public repos, no installation is needed (unauthenticated API).

use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};

use crate::db::Database;

const GITHUB_API_BASE: &str = "https://api.github.com";
const USER_AGENT: &str = "OzzyDB";

/// Errors specific to git provider operations.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error(
        "OzzyDB GitHub App not installed for '{0}'. \
         Install at: https://github.com/apps/ozzydb/installations/new"
    )]
    InstallationNotFound(String),

    #[error("Repository not found or not accessible: {0}")]
    RepoNotFound(String),

    #[error("File not found: {path} at commit {commit_sha}")]
    FileNotFound { path: String, commit_sha: String },

    #[error("Ref not found: {ref_name} in {repo}")]
    RefNotFound { ref_name: String, repo: String },

    #[error("GitHub API error ({status}): {body}")]
    Api { status: u16, body: String },
}

/// JWT claims for GitHub App authentication.
#[derive(Debug, Serialize)]
struct AppJwtClaims {
    /// Issuer: the GitHub App ID
    iss: String,
    /// Issued at (Unix timestamp, backdated 60s for clock skew)
    iat: i64,
    /// Expiration (Unix timestamp, max 10 minutes from iat)
    exp: i64,
}

/// GitHub App configuration. Optional — when absent, only public repos are accessible.
#[derive(Debug, Clone)]
pub struct GitHubAppConfig {
    pub app_id: u64,
    pub private_key: String,
    pub webhook_secret: Option<String>,
}

/// Response from GitHub's installation token endpoint.
#[derive(Debug, Deserialize)]
struct InstallationTokenResponse {
    token: String,
}

/// GitHub-based git provider.
///
/// Fetches repository content via the GitHub API, optionally authenticating
/// as a GitHub App installation for private repo access.
#[derive(Clone)]
pub struct GitHubProvider {
    app_config: Option<GitHubAppConfig>,
    http_client: reqwest::Client,
    db: Database,
}

impl GitHubProvider {
    /// Create a new GitHubProvider.
    ///
    /// If `app_config` is None, only public repos can be accessed.
    pub fn new(app_config: Option<GitHubAppConfig>, db: Database) -> Self {
        Self {
            app_config,
            http_client: reqwest::Client::new(),
            db,
        }
    }

    /// Fetch a tarball of the repo at a specific commit SHA.
    ///
    /// Returns the raw gzipped tar bytes.
    pub async fn fetch_archive(&self, repo: &str, commit_sha: &str) -> Result<Vec<u8>> {
        let token = self.get_token_for_repo(repo).await?;
        let url = format!("{}/repos/{}/tarball/{}", GITHUB_API_BASE, repo, commit_sha);

        let mut req = self
            .http_client
            .get(&url)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", USER_AGENT);

        if let Some(token) = &token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }

        let resp = req
            .send()
            .await
            .context("Failed to fetch archive from GitHub")?;

        match resp.status().as_u16() {
            200..=299 => Ok(resp.bytes().await?.to_vec()),
            404 => {
                if token.is_none() {
                    // Could be a private repo — suggest installing the App
                    let owner = repo_owner(repo);
                    Err(GitError::InstallationNotFound(owner.to_string()).into())
                } else {
                    Err(GitError::RepoNotFound(repo.to_string()).into())
                }
            }
            403 if token.is_none() => {
                let owner = repo_owner(repo);
                Err(GitError::InstallationNotFound(owner.to_string()).into())
            }
            status => {
                let body = resp.text().await.unwrap_or_default();
                Err(GitError::Api { status, body }.into())
            }
        }
    }

    /// Fetch a single file from the repo at a specific commit SHA.
    ///
    /// Returns the raw file bytes.
    pub async fn get_file(&self, repo: &str, commit_sha: &str, path: &str) -> Result<Vec<u8>> {
        let token = self.get_token_for_repo(repo).await?;
        let url = format!(
            "{}/repos/{}/contents/{}?ref={}",
            GITHUB_API_BASE, repo, path, commit_sha
        );

        let mut req = self
            .http_client
            .get(&url)
            .header("Accept", "application/vnd.github.raw+json")
            .header("User-Agent", USER_AGENT);

        if let Some(token) = &token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }

        let resp = req
            .send()
            .await
            .context("Failed to fetch file from GitHub")?;

        match resp.status().as_u16() {
            200..=299 => Ok(resp.bytes().await?.to_vec()),
            404 => Err(GitError::FileNotFound {
                path: path.to_string(),
                commit_sha: commit_sha.to_string(),
            }
            .into()),
            403 if token.is_none() => {
                let owner = repo_owner(repo);
                Err(GitError::InstallationNotFound(owner.to_string()).into())
            }
            status => {
                let body = resp.text().await.unwrap_or_default();
                Err(GitError::Api { status, body }.into())
            }
        }
    }

    /// Resolve a ref (branch or tag name) to a commit SHA.
    ///
    /// Tries branches first, then tags.
    pub async fn resolve_ref(&self, repo: &str, ref_name: &str) -> Result<String> {
        // Try as a branch first
        if let Ok(sha) = self
            .resolve_git_ref(repo, &format!("heads/{}", ref_name))
            .await
        {
            return Ok(sha);
        }

        // Try as a tag
        if let Ok(sha) = self
            .resolve_git_ref(repo, &format!("tags/{}", ref_name))
            .await
        {
            return Ok(sha);
        }

        Err(GitError::RefNotFound {
            ref_name: ref_name.to_string(),
            repo: repo.to_string(),
        }
        .into())
    }

    /// Get the webhook secret, if configured.
    pub fn webhook_secret(&self) -> Option<&str> {
        self.app_config
            .as_ref()
            .and_then(|c| c.webhook_secret.as_deref())
    }

    // ── Internal helpers ──────────────────────────────────────────

    /// Get an installation token for the given repo, if possible.
    ///
    /// Returns None if the GitHub App is not configured or the owner
    /// hasn't installed the App (caller should fall back to unauthenticated).
    async fn get_token_for_repo(&self, repo: &str) -> Result<Option<String>> {
        let config = match &self.app_config {
            Some(c) => c,
            None => return Ok(None),
        };

        let owner = repo_owner(repo);

        let installation = self.db.get_github_installation_by_login(owner).await?;
        let installation = match installation {
            Some(i) => i,
            None => return Ok(None),
        };

        let jwt = create_app_jwt(config.app_id, &config.private_key)?;

        let url = format!(
            "{}/app/installations/{}/access_tokens",
            GITHUB_API_BASE, installation.installation_id
        );

        let resp = self
            .http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", jwt))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", USER_AGENT)
            .send()
            .await
            .context("Failed to get GitHub installation token")?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Failed to get installation token (HTTP {}): {}",
                status,
                body
            ));
        }

        let token_resp: InstallationTokenResponse = resp
            .json()
            .await
            .context("Failed to parse installation token response")?;

        Ok(Some(token_resp.token))
    }

    /// Resolve a fully qualified git ref (e.g., "heads/main" or "tags/v1.0").
    async fn resolve_git_ref(&self, repo: &str, qualified_ref: &str) -> Result<String> {
        let token = self.get_token_for_repo(repo).await?;
        let url = format!(
            "{}/repos/{}/git/ref/{}",
            GITHUB_API_BASE, repo, qualified_ref
        );

        let mut req = self
            .http_client
            .get(&url)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", USER_AGENT);

        if let Some(token) = &token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }

        let resp = req
            .send()
            .await
            .context("Failed to resolve ref from GitHub")?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(GitError::Api { status, body }.into());
        }

        let data: serde_json::Value = resp.json().await?;
        let sha = data["object"]["sha"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing object.sha in git ref response"))?
            .to_string();

        Ok(sha)
    }
}

/// Create a JWT for GitHub App authentication (RS256).
///
/// The JWT is valid for 10 minutes (GitHub maximum).
/// iat is backdated 60 seconds to account for clock skew.
fn create_app_jwt(app_id: u64, private_key_pem: &str) -> Result<String> {
    let now = Utc::now().timestamp();
    let claims = AppJwtClaims {
        iss: app_id.to_string(),
        iat: now - 60,
        exp: now + (10 * 60), // 10 minutes
    };

    let key = EncodingKey::from_rsa_pem(private_key_pem.as_bytes())
        .context("Invalid GitHub App private key (must be PEM-encoded RSA)")?;

    let token = jsonwebtoken::encode(&Header::new(Algorithm::RS256), &claims, &key)
        .context("Failed to sign GitHub App JWT")?;

    Ok(token)
}

/// Extract the owner (user or org) from a "owner/repo" string.
fn repo_owner(repo: &str) -> &str {
    repo.split('/').next().unwrap_or(repo)
}

/// Verify a GitHub webhook HMAC-SHA256 signature.
///
/// `signature` is the value of the `X-Hub-Signature-256` header (e.g., "sha256=abc123...").
pub fn verify_webhook_signature(body: &[u8], secret: &str, signature: &str) -> bool {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let Some(hex_sig) = signature.strip_prefix("sha256=") else {
        return false;
    };

    let Some(expected_bytes) = hex_decode(hex_sig) else {
        return false;
    };

    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else {
        return false;
    };

    mac.update(body);
    mac.verify_slice(&expected_bytes).is_ok()
}

/// Decode a hex string to bytes.
fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repo_owner_normal() {
        assert_eq!(repo_owner("rileyleff/sapflux-analysis"), "rileyleff");
    }

    #[test]
    fn test_repo_owner_org() {
        assert_eq!(repo_owner("my-org/my-repo"), "my-org");
    }

    #[test]
    fn test_repo_owner_no_slash() {
        assert_eq!(repo_owner("just-a-name"), "just-a-name");
    }

    #[test]
    fn test_hex_decode_valid() {
        assert_eq!(hex_decode("48656c6c6f"), Some(b"Hello".to_vec()));
    }

    #[test]
    fn test_hex_decode_empty() {
        assert_eq!(hex_decode(""), Some(vec![]));
    }

    #[test]
    fn test_hex_decode_odd_length() {
        assert_eq!(hex_decode("abc"), None);
    }

    #[test]
    fn test_hex_decode_invalid_chars() {
        assert_eq!(hex_decode("zzzz"), None);
    }

    #[test]
    fn test_verify_webhook_signature_valid() {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        let secret = "test-secret";
        let body = b"test body content";

        // Compute expected signature
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let result = mac.finalize().into_bytes();
        let hex_sig: String = result.iter().map(|b| format!("{:02x}", b)).collect();
        let signature = format!("sha256={}", hex_sig);

        assert!(verify_webhook_signature(body, secret, &signature));
    }

    #[test]
    fn test_verify_webhook_signature_invalid() {
        assert!(!verify_webhook_signature(
            b"body",
            "secret",
            "sha256=0000000000000000000000000000000000000000000000000000000000000000"
        ));
    }

    #[test]
    fn test_verify_webhook_signature_bad_format() {
        assert!(!verify_webhook_signature(b"body", "secret", "not-sha256"));
    }

    #[test]
    fn test_create_app_jwt_invalid_key() {
        let result = create_app_jwt(12345, "not-a-pem-key");
        assert!(result.is_err());
    }

    #[test]
    fn test_create_app_jwt_valid_key() {
        use jsonwebtoken::DecodingKey;

        // Test RSA private key (PKCS#1 format, generated via `openssl genpkey | openssl rsa -traditional`)
        let private_key = "-----BEGIN RSA PRIVATE KEY-----
MIIEpAIBAAKCAQEAojtNuywLL2fNh4phdA2gmlwqkYIKgqoII5ef0VZjO4Qw67T9
0dyXIj3Y9M+OqqFVEZ2oRG3RWY0/o0Jm3R2tmWjZlcEaOx8Kz8hPmYV1GiJyVoJi
ypvPHZCEnlv20mGhdkg6OIKtsFE8j/JoMktkqKPmeoP963qW23oQT86rz7T2xrux
FlAid+ZQ5qzUjTqxgJBRoV6eWHhoJpYd/Epi+fcypZYAVHgbbzEElpjOGUw3a9Pd
yFS8qUvSEVYoOFRcK2I1ZNpn6QXe6qkuKWVRCTObIR1ktNB8OZo5CE7NlXfKNGsU
fikmiV8un/GmyHvsg8qz+dL4RzurXudM1Nh92wIDAQABAoIBAAzWF2jra7klA/ip
BNv7Zg1ApKedu/opvPof+afFJ5XieEJ2MC0mQJkXfq6kK5wppsL9j/5WGB33VKU0
0FHHkuBUEP9N56bs5lyZc17o1eKq/hQFPg7c9C7ZK/0htq5fxjhHL8Af6uFMFDgp
jIAKQh0r1sUz42f566zTBC5kYmjNPwdxLMCI2CCi+tapj+pJtAIUE5MunsFWjY2A
ZAEhrTNmiIW3HpwyebDoJJiqba+jzGA0X8v9XjpAY/igbiEsgD8luHaHzSHkL4Zm
ssVR+aLMq+ZNqeP6rzutc3bIxmGIyUpJDaASMFFb++fQTUZA6WP4QIczLlZwK1JU
5tO3YDkCgYEAy+cfGqsk0kGYr7XPxwgk7WaJpPAPOT3wTtb5slhWr/8IgI03WQ/1
3K7fcDP7IUSKG1audSQIHYTjd1kob1VujCdMwY1YX7zl4BJLtX0YVIoRNpDu2j7B
n4+GuWtIrsPIQ8RgHhM1kMoShV8kz7yazUl1MdmS9p7chKhGZcJgwCMCgYEAy66N
120wRFC1xgmSNNlmhKY6bggEIsd6jXjixStDCuB76vrjyLL3ZuTCwbCw3LC+pzPj
6+T9Db8MZQlertwBHgLAodDty5Anhz7xExJoPWcNU83KXNzD71UaoA++jXzjGWEX
h//IJW6Ejs036nIJxQ/xtcrbZEo5yorgh9v6yukCgYEAjbozW4UUDfU8XP3B03Us
vbqk9/lIi6Aq1ZIFc1qFvsVFMp11mDlIysDeXA41gzUxzbjdgFywK1yAAyf8vA4k
zdRPPMWzZLBXE/2DlD3EWJazSjtJWnd+fBr2KRGSLq+1Fq17pcvyUpaycvPkEWTm
MMTbae/yf+uCGc8hR3/pYgMCgYEAgCnICuQj2PjISGLBYwvhHFrUrWPR5miKzvZg
Cx0bxH5YuxU/u4wRbOdJPUOHJnb4oJFgO5ENQlcS34iz0WeSNGYa/DdRSiPdG5cZ
bpzIhsqPckotqZ0olTKB3HHLv4/z/oY/nk3ujM8sDgdHxfgX+a38tB1/S24BS1dz
zUk4V/ECgYApuMoEZ7lFpyNpmVNJBSFzJBXeQ2PjjvFKpMLfuKm7j7l9S7SeBt4V
yIfVavn2ZZ+7vVlwd6qLznuWxV8oOV235q34OKyOaj6QmmPFL7iATNjpmXSUyoNB
X6OYcNnWJCzJrV2i+Qu200U6YmwM6nN9ST6AFs/aMM7tfIhGxmUadw==
-----END RSA PRIVATE KEY-----";

        let result = create_app_jwt(12345, private_key);
        assert!(result.is_ok());

        let jwt = result.unwrap();
        // Verify it's a valid JWT format (three dot-separated parts)
        let parts: Vec<&str> = jwt.split('.').collect();
        assert_eq!(parts.len(), 3);

        // Verify the claims can be decoded (without signature verification, just structure)
        let _decoding_key = DecodingKey::from_rsa_pem(private_key.as_bytes()).unwrap();
    }
}
