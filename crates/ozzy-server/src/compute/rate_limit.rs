//! Rate limiting for compute jobs.
//!
//! Enforces global and per-user concurrent job caps before allowing new job creation.

use anyhow::Result;
use uuid::Uuid;

use crate::config::RateLimitConfig;
use crate::db::Database;

/// Check rate limits before creating a new job.
///
/// Returns Ok(()) if the user is within limits, or Err with a descriptive message
/// if any limit is exceeded.
pub async fn check_limits(
    db: &Database,
    config: &RateLimitConfig,
    user_id: Uuid,
) -> Result<(), RateLimitError> {
    // Check per-user limit
    if config.per_user_max_concurrent > 0 {
        let user_active = db
            .count_active_jobs_for_user(user_id)
            .await
            .map_err(|e| RateLimitError::Internal(e.to_string()))?;

        if user_active >= config.per_user_max_concurrent as i64 {
            return Err(RateLimitError::PerUserLimitExceeded {
                current: user_active,
                limit: config.per_user_max_concurrent,
            });
        }
    }

    // Check global limit
    if config.global_max_concurrent > 0 {
        let global_active = db
            .count_active_jobs_global()
            .await
            .map_err(|e| RateLimitError::Internal(e.to_string()))?;

        if global_active >= config.global_max_concurrent as i64 {
            return Err(RateLimitError::GlobalLimitExceeded {
                current: global_active,
                limit: config.global_max_concurrent,
            });
        }
    }

    Ok(())
}

/// Rate limit error types.
#[derive(Debug, thiserror::Error)]
pub enum RateLimitError {
    #[error("Per-user concurrent job limit exceeded ({current}/{limit})")]
    PerUserLimitExceeded { current: i64, limit: u32 },

    #[error("Global concurrent job limit exceeded ({current}/{limit})")]
    GlobalLimitExceeded { current: i64, limit: u32 },

    #[error("Internal error checking rate limits: {0}")]
    Internal(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit_error_display() {
        let err = RateLimitError::PerUserLimitExceeded {
            current: 5,
            limit: 5,
        };
        assert_eq!(
            err.to_string(),
            "Per-user concurrent job limit exceeded (5/5)"
        );

        let err = RateLimitError::GlobalLimitExceeded {
            current: 20,
            limit: 20,
        };
        assert_eq!(
            err.to_string(),
            "Global concurrent job limit exceeded (20/20)"
        );
    }
}
