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
    if let Some(r2) = &config.r2 {
        tracing::info!("R2 storage: {}/{}", r2.endpoint, r2.bucket);
        tracing::info!("Local cache at {}", config.cache_dir);
    } else {
        tracing::info!("Running in local-only mode (no R2 configured)");
        tracing::info!("Local storage at {}", config.cache_dir);
    }

    if !config.allowed_logins.is_empty() {
        tracing::info!(
            "Registration restricted to: {}",
            config.allowed_logins.join(", ")
        );
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

    // Initialize compute backend
    let compute = ozzy_server::compute::BackendSelector::from_config(
        &config.compute,
        config.fly.as_ref(),
        Some(&storage),
    );
    match &compute {
        Some(ozzy_server::compute::BackendSelector::Fly(_)) => {
            tracing::info!(
                "Compute enabled (Fly backend, app: {}, region: {})",
                config.fly.as_ref().unwrap().app_name,
                config.fly.as_ref().unwrap().region,
            );
        }
        Some(ozzy_server::compute::BackendSelector::Docker(_)) => {
            tracing::info!(
                "Compute enabled (Docker backend, tmpdir: {})",
                config.compute.tmpdir
            );
        }
        None => {
            tracing::info!("Compute disabled");
        }
    }

    // Build application state
    let state = AppState {
        config: Arc::new(config.clone()),
        db,
        storage,
        git,
        compute,
    };

    // Spawn periodic orphan cleanup for Fly backend
    if let Some(fly) = state.compute.as_ref().and_then(|c| c.as_fly()) {
        let fly_clone = fly.clone();
        tokio::spawn(async move {
            let interval = std::time::Duration::from_secs(5 * 60); // every 5 minutes
            let max_age = std::time::Duration::from_secs(30 * 60); // 30 min threshold
            loop {
                tokio::time::sleep(interval).await;
                if let Err(e) = fly_clone.cleanup_orphans(max_age).await {
                    tracing::error!("Fly orphan cleanup failed: {}", e);
                }
            }
        });
        tracing::info!("Fly orphan cleanup task started (every 5 min, max age 30 min)");
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
