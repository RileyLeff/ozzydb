//! API endpoint integration tests.
//!
//! Tests the Axum HTTP routes directly using tower::ServiceExt.
//! The health endpoint test runs without external dependencies.
//! Other tests require DATABASE_URL.

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use ozzy_server::{AppState, Config, ContentStorage, Database, api};
use sqlx::postgres::PgPoolOptions;
use std::env;
use std::sync::Arc;
use tower::ServiceExt;

/// Build a test app with real DB + storage (returns None if credentials unavailable).
async fn build_test_app() -> Option<Router> {
    let db_url = env::var("DATABASE_URL").ok()?;

    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&db_url)
        .await
        .ok()?;

    sqlx::migrate!("./migrations").run(&pool).await.ok()?;

    let cache_dir = tempfile::tempdir().ok()?.into_path();

    let config = Config {
        bind_address: "127.0.0.1:0".to_string(),
        database_url: db_url,
        db_max_connections: 2,
        github_client_id: "test_client_id".to_string(),
        github_client_secret: "test_client_secret".to_string(),
        base_url: "http://localhost:3000".to_string(),
        cache_dir: cache_dir.to_string_lossy().to_string(),
        r2: None,
        max_upload_size_bytes: 104_857_600,
        cors_origins: "*".to_string(),
        allowed_logins: vec![],
        secrets_encryption_key: None,
        github_app: None,
        compute: ozzy_server::config::ComputeConfig {
            enabled: false,
            docker_runtime: None,
            memory_limit: "2g".to_string(),
            cpu_limit: "1".to_string(),
            timeout_secs: 300,
            tmpdir: "/tmp/ozzy-test".to_string(),
            tmpfs_size: "512m".to_string(),
        },
        fly: None,
        rate_limit: ozzy_server::config::RateLimitConfig {
            global_max_concurrent: 0,
            per_user_max_concurrent: 0,
        },
        dev_auto_user: None,
    };

    let storage = ContentStorage::from_config(&config).ok()?;
    let db = Database::new(pool);
    let git = ozzy_server::GitHubProvider::new(None, db.clone());
    let state = AppState {
        config: Arc::new(config),
        db,
        storage,
        git,
        compute: None,
    };

    let app = Router::new().merge(api::router()).with_state(state);

    Some(app)
}

#[tokio::test]
async fn test_health_endpoint() {
    let Some(app) = build_test_app().await else {
        eprintln!("Skipping API tests: DATABASE_URL not set");
        return;
    };

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["status"], "ok");
    assert!(json["version"].is_string());
}

#[tokio::test]
async fn test_health_returns_json() {
    let Some(app) = build_test_app().await else {
        return;
    };

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let content_type = response
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(content_type.contains("application/json"));
}

#[tokio::test]
async fn test_404_for_unknown_route() {
    let Some(app) = build_test_app().await else {
        return;
    };

    let response = app
        .oneshot(
            Request::builder()
                .uri("/nonexistent")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_auth_me_requires_token() {
    let Some(app) = build_test_app().await else {
        return;
    };

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/auth/me")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Should get 401 Unauthorized without a token
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_auth_token_list_requires_token() {
    let Some(app) = build_test_app().await else {
        return;
    };

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/auth/token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
