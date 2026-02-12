//! Docker-based integration tests for the OzzyDB registry server.
//!
//! v2: These tests will be rewritten as v2 API endpoints are implemented.
//! The v1 push/pull protocol has been removed. New tests will cover:
//! - Data atom upload/download
//! - Collection management
//! - Git-referenced commits
//! - Environment management
//! - Compute pipeline (Fly Machines)
//!
//! Requirements: Docker must be running.
//!
//! Run: cargo test -p ozzy-server --test integration_tests -- --ignored

use std::sync::Arc;

use ozzy_server::config::Config;
use ozzy_server::db::Database;
use ozzy_server::storage::ContentStorage;
use ozzy_server::{AppState, api};
use sqlx::postgres::PgPoolOptions;
use std::sync::LazyLock;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

// ========================================================================
// Test Infrastructure
// ========================================================================

/// Shared test server backed by a real Postgres container.
struct TestServer {
    base_url: String,
    client: reqwest::Client,
    db: Database,
    // Keep container and storage dir alive for the test session.
    _container: testcontainers::ContainerAsync<Postgres>,
    _storage_dir: tempfile::TempDir,
}

// Safety: PgPool, reqwest::Client, and ContainerAsync are all Send+Sync.
unsafe impl Send for TestServer {}
unsafe impl Sync for TestServer {}

/// Shared tokio runtime that hosts the test server, PgPool, and Axum server.
static TEST_RT: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
});

static TEST_SERVER: LazyLock<TestServer> = LazyLock::new(|| {
    TEST_RT.block_on(TestServer::start())
});

impl TestServer {
    async fn start() -> Self {
        let _ = tracing_subscriber::fmt()
            .with_env_filter("ozzy_server=debug,sqlx=warn")
            .try_init();

        let container = Postgres::default()
            .start()
            .await
            .expect("Failed to start PostgreSQL container (is Docker running?)");

        let host_port = container
            .get_host_port_ipv4(5432)
            .await
            .expect("Failed to get Postgres host port");

        let db_url = format!(
            "postgres://postgres:postgres@127.0.0.1:{}/postgres",
            host_port
        );

        let pool = PgPoolOptions::new()
            .max_connections(50)
            .connect(&db_url)
            .await
            .expect("Failed to connect to test database");

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("Failed to run migrations");

        let db = Database::new(pool);

        let storage_dir = tempfile::tempdir().expect("Failed to create temp dir");

        let config = Config {
            bind_address: "127.0.0.1:0".to_string(),
            database_url: db_url,
            db_max_connections: 50,
            github_client_id: "test_client_id".to_string(),
            github_client_secret: "test_client_secret".to_string(),
            base_url: "http://localhost:3000".to_string(),
            cache_dir: storage_dir.path().to_string_lossy().to_string(),
            r2: None,
            max_upload_size_bytes: 104_857_600,
            cors_origins: "*".to_string(),
            allowed_logins: vec![],
        };

        let storage =
            ContentStorage::from_config(&config).expect("Failed to create content storage");

        let state = AppState {
            config: Arc::new(config),
            db: db.clone(),
            storage,
        };

        let app = axum::Router::new()
            .merge(api::router())
            .with_state(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind test server");
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{}", addr);

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = reqwest::Client::new();

        TestServer {
            base_url,
            client,
            db,
            _container: container,
            _storage_dir: storage_dir,
        }
    }

    /// Create a test user with an account-scoped API token. Returns (username, bearer_token).
    async fn create_test_user(&self, suffix: &str) -> (String, String) {
        let github_id = (rand::random::<i64>() & i64::MAX);
        let username = format!("testuser_{}", suffix);
        let user = self
            .db
            .upsert_user_from_github(github_id, &username, None, None)
            .await
            .expect("Failed to create test user");

        let (plaintext, token_hash) = ozzy_server::auth::tokens::generate_api_token();

        self.db
            .create_token(
                user.id,
                &format!("test-token-{}", suffix),
                &token_hash,
                "account",
                None,
                None,
            )
            .await
            .expect("Failed to create test token");

        (username, plaintext)
    }
}

// ========================================================================
// Tests
// ========================================================================

/// Verify that the test server starts up and health check works.
#[test]
#[ignore] // Requires Docker
fn test_server_health() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        let resp = s.client
            .get(format!("{}/health", s.base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["status"], "ok");
    });
}

/// Concurrent cli-session token upserts for the same user.
/// After concurrent operations, exactly one cli-session token should exist.
#[test]
#[ignore] // Requires Docker
fn test_concurrent_token_upsert() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        let github_id = (rand::random::<i64>() & i64::MAX);
        let username = format!("token_race_{}", github_id);
        let user = s
            .db
            .upsert_user_from_github(github_id, &username, None, None)
            .await
            .unwrap();

        let n = 10;
        let mut handles = Vec::new();

        for i in 0..n {
            let db = s.db.clone();
            let user_id = user.id;
            handles.push(tokio::spawn(async move {
                let (_plaintext, token_hash) = ozzy_server::auth::tokens::generate_api_token();
                let expires = chrono::Utc::now() + chrono::Duration::days(90);
                db.upsert_session_token(user_id, "cli-session", &token_hash, expires)
                    .await
                    .unwrap();
                i
            }));
        }

        let mut results = Vec::new();
        for handle in handles {
            results.push(handle.await.unwrap());
        }
        assert_eq!(results.len(), n);

        let tokens = s.db.list_user_tokens(user.id).await.unwrap();
        let cli_tokens: Vec<_> = tokens
            .iter()
            .filter(|t| t.name == "cli-session")
            .collect();
        assert_eq!(
            cli_tokens.len(),
            1,
            "Expected exactly 1 cli-session token after {} concurrent upserts, found {}",
            n,
            cli_tokens.len()
        );
    });
}

/// Error responses should use the {error, message} format.
#[test]
#[ignore] // Requires Docker
fn test_error_responses_use_consistent_format() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        // 401: No auth token on /auth/me
        let resp = s.client
            .get(format!("{}/api/v1/auth/me", s.base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 401);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert!(body.get("error").is_some(), "401 response should have 'error' field: {:?}", body);
        assert!(body.get("message").is_some(), "401 response should have 'message' field: {:?}", body);
    });
}

/// Project-scoped token should not be able to access account-wide endpoints.
#[test]
#[ignore] // Requires Docker
fn test_project_scoped_token_cannot_access_account_endpoints() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        // Create a user with a project-scoped token
        let github_id = (rand::random::<i64>() & i64::MAX);
        let username = format!("testuser_scoped_{}", github_id);
        let user = s
            .db
            .upsert_user_from_github(github_id, &username, None, None)
            .await
            .unwrap();

        // Create a project for the scoped token
        let project = s
            .db
            .get_or_create_project(user.id, &format!("proj-{}", github_id), "private")
            .await
            .unwrap();

        let (scoped_plaintext, scoped_hash) = ozzy_server::auth::tokens::generate_api_token();
        s.db
            .create_token(
                user.id,
                "scoped-token",
                &scoped_hash,
                &format!("project:{}/{}", username, project.slug),
                Some(project.id),
                None,
            )
            .await
            .unwrap();

        // GET /auth/me should be rejected for project-scoped tokens
        let resp = s.client
            .get(format!("{}/api/v1/auth/me", s.base_url))
            .header("Authorization", format!("Bearer {}", scoped_plaintext))
            .send()
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            403,
            "Project-scoped token should not access /auth/me, got {}",
            resp.status()
        );

        // GET /auth/token should also be rejected
        let resp = s.client
            .get(format!("{}/api/v1/auth/token", s.base_url))
            .header("Authorization", format!("Bearer {}", scoped_plaintext))
            .send()
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            403,
            "Project-scoped token should not access /auth/token, got {}",
            resp.status()
        );

        // Verify that an account-scoped token CAN access these
        let (owner_plaintext, owner_hash) = ozzy_server::auth::tokens::generate_api_token();
        s.db
            .create_token(
                user.id,
                "account-token",
                &owner_hash,
                "account",
                None,
                None,
            )
            .await
            .unwrap();

        let resp = s.client
            .get(format!("{}/api/v1/auth/me", s.base_url))
            .header("Authorization", format!("Bearer {}", owner_plaintext))
            .send()
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            200,
            "Account token should access /auth/me, got {}",
            resp.status()
        );
    });
}
