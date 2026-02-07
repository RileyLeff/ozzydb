//! API token generation and validation utilities.

use anyhow::Result;
use ozzy_core::hash;
use rand::Rng;

/// Generate a new API token with the ozzy_ prefix.
/// Returns (plaintext_token, token_hash).
pub fn generate_api_token() -> (String, String) {
    // Generate 32 random bytes
    let mut rng = rand::rng();
    let random_bytes: [u8; 32] = rng.random();

    // Encode as base64url (no padding)
    let token_body = base64_url_encode(&random_bytes);

    // Create the full token with prefix
    let plaintext = format!("ozzy_{}", token_body);

    // Hash the token for storage
    let token_hash = hash::blake3_hash(plaintext.as_bytes());

    (plaintext, token_hash)
}

/// Hash a token for lookup.
pub fn hash_token(token: &str) -> String {
    hash::blake3_hash(token.as_bytes())
}

/// Validate token format.
pub fn validate_token_format(token: &str) -> Result<()> {
    if !token.starts_with("ozzy_") {
        anyhow::bail!("Invalid token format: must start with 'ozzy_'");
    }

    let body = &token[5..];
    if body.len() < 32 {
        anyhow::bail!("Invalid token format: token too short");
    }

    Ok(())
}

/// Base64url encoding without padding.
fn base64_url_encode(bytes: &[u8]) -> String {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_token() {
        let (token, hash) = generate_api_token();

        assert!(token.starts_with("ozzy_"));
        assert!(token.len() > 40);
        assert_eq!(hash.len(), 64); // BLAKE3 hex
    }

    #[test]
    fn test_hash_consistency() {
        let (token, hash1) = generate_api_token();
        let hash2 = hash_token(&token);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_validate_format() {
        assert!(validate_token_format("ozzy_abcdefghijklmnopqrstuvwxyz123456").is_ok());
        assert!(validate_token_format("invalid_token").is_err());
        assert!(validate_token_format("ozzy_short").is_err());
    }
}
