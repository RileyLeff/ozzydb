//! OzzyDB Registry Server
//!
//! v2: Data management platform for scientific computing.

use anyhow::Result;
use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderValue, Method};
use ozzy_server::{
    AppState, api, config::Config, db::Database, git, git::GitHubProvider, storage::ContentStorage,
};
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ozzy_server=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Load configuration
    let config = Config::from_env()?;
    tracing::info!("Starting OzzyDB Registry Server");
    tracing::info!("Binding to {}", config.bind_address);

    // Connect to database
    let pool = PgPoolOptions::new()
        .max_connections(config.db_max_connections)
        .connect(&config.database_url)
        .await?;
    tracing::info!("Connected to PostgreSQL");

    // Run migrations
    sqlx::migrate!("./migrations").run(&pool).await?;
    tracing::info!("Database migrations complete");

    // Initialize storage
    let storage = ContentStorage::from_config(&config)?;
    tracing::info!("R2 storage: {}/{}", config.r2.endpoint, config.r2.bucket);

    if !config.allowed_logins.is_empty() {
        tracing::info!(
            "Registration restricted to: {}",
            config.allowed_logins.join(", ")
        );
    }

    // Dev mode: auto-create user and print auth token
    if let Some(ref username) = config.dev_auto_user {
        let db_tmp = Database::new(pool.clone());
        let user = sqlx::query_as::<_, ozzy_server::db::models::User>(
            r#"
            INSERT INTO users (id, username, display_name, is_admin)
            VALUES ($1, $2, $3, true)
            ON CONFLICT (username) DO UPDATE SET
                display_name = EXCLUDED.display_name,
                is_admin = EXCLUDED.is_admin
            RETURNING *
            "#,
        )
        .bind(uuid::Uuid::new_v4())
        .bind(username)
        .bind(username)
        .fetch_one(&pool)
        .await?;

        // Clean up old dev tokens, then create a fresh one
        sqlx::query("DELETE FROM api_tokens WHERE user_id = $1 AND name = 'dev-token'")
            .bind(user.id)
            .execute(&pool)
            .await?;
        let raw_token = format!("ozzy_dev_{}", uuid::Uuid::new_v4().simple());
        let token_hash = ozzy_core::hash::blake3_hash(raw_token.as_bytes());
        db_tmp
            .create_token(user.id, "dev-token", &token_hash, "account", None, None)
            .await?;

        tracing::info!("=== DEV MODE ===");
        tracing::info!("Auto-created user: {} (admin)", username);
        tracing::info!("Auth token: {}", raw_token);
        tracing::info!("================");
    }

    // Initialize git provider
    let db = Database::new(pool);
    let git_app_config = config.github_app.as_ref().map(|app| {
        tracing::info!("GitHub App configured (ID: {})", app.app_id);
        git::github::GitHubAppConfig {
            app_id: app.app_id,
            private_key: app.private_key.clone(),
            webhook_secret: app.webhook_secret.clone(),
        }
    });
    if git_app_config.is_none() {
        tracing::info!("GitHub App not configured — only public repos accessible");
    }
    let git = GitHubProvider::new(git_app_config, db.clone());

    // Initialize compute registry
    let compute = ozzy_server::compute::ComputeRegistry::from_config(
        &config.compute,
        config.docker.as_ref(),
        config.fly.as_ref(),
    );
    if compute.is_enabled() {
        tracing::info!(
            "Compute backends configured: {} (selected: {})",
            compute.configured_backend_names().join(", "),
            compute.selected_backend_name().unwrap_or("none"),
        );
    } else {
        tracing::info!("Compute disabled (no providers configured)");
    }

    // Build application state
    let state = AppState {
        config: Arc::new(config.clone()),
        db,
        storage,
        git,
        registry_snapshots: ozzy_server::registry::RegistrySnapshotCache::default(),
        compute,
    };

    // Spawn periodic orphan cleanup for Fly backend
    if let Some(fly) = state.compute.fly_backend() {
        let fly_clone = fly.clone();
        // Derive max_age from compute timeout: 2x timeout + 10 min buffer
        // This ensures we never destroy a machine that's still within its allowed execution window
        let max_age_secs = config.compute.timeout_secs * 2 + 600;
        tokio::spawn(async move {
            let interval = std::time::Duration::from_secs(5 * 60); // every 5 minutes
            let max_age = std::time::Duration::from_secs(max_age_secs);
            loop {
                tokio::time::sleep(interval).await;
                if let Err(e) = fly_clone.cleanup_orphans(max_age).await {
                    tracing::error!("Fly orphan cleanup failed: {}", e);
                }
            }
        });
        tracing::info!(
            "Fly orphan cleanup task started (every 5 min, max age {}s)",
            max_age_secs
        );
    }

    // Build router
    let app = Router::new()
        .merge(api::router())
        .layer(DefaultBodyLimit::max(config.max_upload_size_bytes as usize))
        .layer(TraceLayer::new_for_http())
        .layer({
            let cors = CorsLayer::new()
                .allow_methods([Method::GET, Method::POST, Method::DELETE, Method::OPTIONS])
                .allow_headers(Any);
            if config.cors_origins == "*" {
                cors.allow_origin(Any)
            } else {
                let origins: Vec<HeaderValue> = config
                    .cors_origins
                    .split(',')
                    .filter_map(|s| s.trim().parse().ok())
                    .collect();
                cors.allow_origin(origins)
            }
        })
        .with_state(state);

    // Start server
    let listener = tokio::net::TcpListener::bind(&config.bind_address).await?;
    tracing::info!("Server listening on {}", config.bind_address);

    axum::serve(listener, app).await?;

    Ok(())
}
