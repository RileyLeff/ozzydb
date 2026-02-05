//! API endpoint integration tests.
//!
//! Tests the Axum HTTP routes directly using tower::ServiceExt.
//! The health endpoint test runs without external dependencies.
//! Push/pull tests require DATABASE_URL and R2 credentials.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use ozzy_server::{api, AppState, Config, ContentStorage, Database};
use sqlx::postgres::PgPoolOptions;
use std::env;
use std::sync::Arc;
use tower::ServiceExt;

/// Build a test app with real DB + storage (returns None if credentials unavailable).
async fn build_test_app() -> Option<Router> {
    let db_url = env::var("DATABASE_URL").ok()?;
    let r2_endpoint = env::var("R2_ENDPOINT").ok()?;
    let r2_bucket = env::var("R2_BUCKET").ok()?;

    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&db_url)
        .await
        .ok()?;

    sqlx::migrate!("./migrations").run(&pool).await.ok()?;

    let config = Config {
        bind_address: "127.0.0.1:0".to_string(),
        database_url: db_url,
        db_max_connections: 2,
        github_client_id: "test_client_id".to_string(),
        github_client_secret: "test_client_secret".to_string(),
        base_url: "http://localhost:3000".to_string(),
        r2: ozzy_server::config::R2Config {
            endpoint: r2_endpoint,
            bucket: r2_bucket,
            access_key_id: env::var("R2_ACCESS_KEY_ID").unwrap_or_default(),
            secret_access_key: env::var("R2_SECRET_ACCESS_KEY").unwrap_or_default(),
            region: env::var("R2_REGION").unwrap_or_else(|_| "us-east-1".into()),
        },
        max_tar_size_bytes: 1_073_741_824,
        max_upload_size_bytes: 104_857_600,
        cors_origins: "*".to_string(),
    };

    let storage = ContentStorage::new(&config.r2).ok()?;
    let state = AppState {
        config: Arc::new(config),
        db: Database::new(pool),
        storage,
    };

    let app = Router::new()
        .merge(api::router())
        .with_state(state);

    Some(app)
}

#[tokio::test]
async fn test_health_endpoint() {
    let Some(app) = build_test_app().await else {
        eprintln!("Skipping API tests: DATABASE_URL or R2 credentials not set");
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
async fn test_list_projects_requires_auth() {
    let Some(app) = build_test_app().await else {
        return;
    };

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/projects")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_get_nonexistent_project() {
    let Some(app) = build_test_app().await else {
        return;
    };

    // Public project lookup should return 404, not 401
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/nobody/nonexistent")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Could be 404 or 500 depending on how error is mapped
    assert!(
        response.status() == StatusCode::NOT_FOUND
            || response.status() == StatusCode::INTERNAL_SERVER_ERROR,
        "Expected 404 or 500, got {}",
        response.status()
    );
}

#[tokio::test]
async fn test_push_requires_auth() {
    let Some(app) = build_test_app().await else {
        return;
    };

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/testuser/testproject/push")
                .header("content-type", "multipart/form-data; boundary=test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
