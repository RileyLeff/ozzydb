//! OzzyDB Registry Server
//!
//! A content-addressed registry for sharing OzzyDB projects across teams.

use anyhow::Result;
use axum::Router;
use axum::http::{HeaderValue, Method};
use ozzy_server::{AppState, api, config::Config, db::Database, storage::ContentStorage};
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

    // Initialize storage (local NVMe primary, optional R2 redundancy)
    let storage = ContentStorage::from_config(&config)?;
    tracing::info!("Local storage initialized at {}", config.local_storage_path);
    if let Some(r2) = &config.r2 {
        tracing::info!("R2 redundancy enabled: {}/{}", r2.endpoint, r2.bucket);
    } else {
        tracing::info!("R2 redundancy disabled");
    }

    // Build application state
    let state = AppState {
        config: Arc::new(config.clone()),
        db: Database::new(pool),
        storage,
    };

    // Build router
    let app = Router::new()
        .merge(api::router())
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
