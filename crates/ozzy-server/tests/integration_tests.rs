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
use testcontainers::core::ImageExt;
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
            .with_tag("17-alpine")
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

// ========================================================================
// Data API Tests
// ========================================================================

/// Helper: create a user + project + token, returning (username, slug, bearer_token).
async fn setup_data_test(s: &TestServer, suffix: &str) -> (String, String, String) {
    let github_id = rand::random::<i64>() & i64::MAX;
    let username = format!("datauser_{}", suffix);
    let slug = format!("dataproj_{}", suffix);
    let user = s
        .db
        .upsert_user_from_github(github_id, &username, None, None)
        .await
        .unwrap();

    s.db.get_or_create_project(user.id, &slug, "private")
        .await
        .unwrap();

    let (plaintext, token_hash) = ozzy_server::auth::tokens::generate_api_token();
    s.db.create_token(user.id, "test-token", &token_hash, "account", None, None)
        .await
        .unwrap();

    (username, slug, plaintext)
}

/// Full data atom lifecycle: upload → list → get → download → describe → metadata → yank → download returns 410.
#[test]
#[ignore] // Requires Docker
fn test_data_atom_lifecycle() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        let (owner, slug, token) = setup_data_test(s, &format!("lifecycle_{}", rand::random::<u32>())).await;

        // 1. Upload a CSV data atom
        let csv_content = b"col_a,col_b\n1,hello\n2,world\n";
        let file_part = reqwest::multipart::Part::bytes(csv_content.to_vec())
            .file_name("readings.csv")
            .mime_str("text/csv")
            .unwrap();
        let form = reqwest::multipart::Form::new()
            .part("file", file_part)
            .text("project", format!("{}/{}", owner, slug))
            .text("description", "Test CSV data");

        let resp = s.client
            .post(format!("{}/api/v1/data/upload", s.base_url))
            .header("Authorization", format!("Bearer {}", token))
            .multipart(form)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200, "Upload failed: {}", resp.text().await.unwrap_or_default());

        // Re-upload to check response (need fresh request)
        let file_part2 = reqwest::multipart::Part::bytes(csv_content.to_vec())
            .file_name("readings.csv")
            .mime_str("text/csv")
            .unwrap();
        let form2 = reqwest::multipart::Form::new()
            .part("file", file_part2)
            .text("project", format!("{}/{}", owner, slug))
            .text("name", "readings2");
        let resp = s.client
            .post(format!("{}/api/v1/data/upload", s.base_url))
            .header("Authorization", format!("Bearer {}", token))
            .multipart(form2)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let upload_body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(upload_body["name"], "readings2");
        assert_eq!(upload_body["content_type"], "text/csv");
        assert!(upload_body["deduplicated"].as_bool().unwrap(), "Same content should be deduplicated");

        // 2. List data atoms
        let resp = s.client
            .get(format!("{}/api/v1/data/{}/{}", s.base_url, owner, slug))
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let list_body: Vec<serde_json::Value> = resp.json().await.unwrap();
        assert_eq!(list_body.len(), 2);

        // 3. Get data atom detail
        let resp = s.client
            .get(format!("{}/api/v1/data/{}/{}/readings", s.base_url, owner, slug))
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let detail: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(detail["name"], "readings");
        assert_eq!(detail["content_type"], "text/csv");
        assert_eq!(detail["yanked"], false);
        // Should have the description metadata we set during upload
        assert_eq!(detail["metadata"]["description"], "Test CSV data");

        // 4. Download data atom
        let resp = s.client
            .get(format!("{}/api/v1/data/{}/{}/readings/download", s.base_url, owner, slug))
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let ct = resp.headers().get("content-type").unwrap().to_str().unwrap();
        assert_eq!(ct, "text/csv");
        let disp = resp.headers().get("content-disposition").unwrap().to_str().unwrap();
        assert!(disp.contains("readings.csv"), "Content-Disposition should have filename");
        let body_bytes = resp.bytes().await.unwrap();
        assert_eq!(&body_bytes[..], csv_content);

        // 5. Describe (append metadata)
        let resp = s.client
            .post(format!("{}/api/v1/data/{}/{}/readings/describe", s.base_url, owner, slug))
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .body(r#"{"field": "source", "value": "sensor-42"}"#)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let desc_body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(desc_body["field"], "source");
        assert_eq!(desc_body["value"], "sensor-42");

        // 6. Get metadata history
        let resp = s.client
            .get(format!("{}/api/v1/data/{}/{}/readings/metadata", s.base_url, owner, slug))
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let meta_body: Vec<serde_json::Value> = resp.json().await.unwrap();
        assert!(meta_body.len() >= 2, "Should have description + source metadata entries");

        // 7. Yank the data atom
        let resp = s.client
            .post(format!("{}/api/v1/data/{}/{}/readings/yank", s.base_url, owner, slug))
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .body(r#"{"reason": "data quality issue"}"#)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let yank_body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(yank_body["yanked"], true);

        // 8. Download after yank should return 410 Gone
        let resp = s.client
            .get(format!("{}/api/v1/data/{}/{}/readings/download", s.base_url, owner, slug))
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 410, "Yanked atom download should return 410 Gone");
    });
}

/// Upload requires authentication.
#[test]
#[ignore] // Requires Docker
fn test_data_upload_requires_auth() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        let file_part = reqwest::multipart::Part::bytes(b"test".to_vec())
            .file_name("test.csv");
        let form = reqwest::multipart::Form::new()
            .part("file", file_part)
            .text("project", "someone/something");

        let resp = s.client
            .post(format!("{}/api/v1/data/upload", s.base_url))
            .multipart(form)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 401);
    });
}

/// List data for nonexistent project returns 404.
#[test]
#[ignore] // Requires Docker
fn test_data_list_nonexistent_project() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        let (_, _, token) = setup_data_test(s, &format!("noproject_{}", rand::random::<u32>())).await;

        let resp = s.client
            .get(format!("{}/api/v1/data/nobody/nonexistent", s.base_url))
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 404);
    });
}

/// Upload with invalid name returns 400.
#[test]
#[ignore] // Requires Docker
fn test_data_upload_invalid_name() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        let (owner, slug, token) = setup_data_test(s, &format!("badname_{}", rand::random::<u32>())).await;

        let file_part = reqwest::multipart::Part::bytes(b"test data".to_vec())
            .file_name("test.csv");
        let form = reqwest::multipart::Form::new()
            .part("file", file_part)
            .text("project", format!("{}/{}", owner, slug))
            .text("name", "has.dot");

        let resp = s.client
            .post(format!("{}/api/v1/data/upload", s.base_url))
            .header("Authorization", format!("Bearer {}", token))
            .multipart(form)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 400);
    });
}

/// Yank with empty reason returns 400.
#[test]
#[ignore] // Requires Docker
fn test_data_yank_empty_reason() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        let (owner, slug, token) = setup_data_test(s, &format!("emptyyank_{}", rand::random::<u32>())).await;

        // Upload first
        let file_part = reqwest::multipart::Part::bytes(b"some data".to_vec())
            .file_name("thing.csv");
        let form = reqwest::multipart::Form::new()
            .part("file", file_part)
            .text("project", format!("{}/{}", owner, slug));

        s.client
            .post(format!("{}/api/v1/data/upload", s.base_url))
            .header("Authorization", format!("Bearer {}", token))
            .multipart(form)
            .send()
            .await
            .unwrap();

        // Yank with empty reason
        let resp = s.client
            .post(format!("{}/api/v1/data/{}/{}/thing/yank", s.base_url, owner, slug))
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .body(r#"{"reason": ""}"#)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 400);
    });
}

// ========================================================================
// Collections API Tests
// ========================================================================

/// Full collection lifecycle: create → add members → get → log → flatten → remove → yank.
#[test]
#[ignore] // Requires Docker
fn test_collection_lifecycle() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        let (owner, slug, token) = setup_data_test(s, &format!("coll_life_{}", rand::random::<u32>())).await;
        let base = format!("{}/api/v1", s.base_url);

        // Upload two data atoms first
        for name in &["alpha", "beta"] {
            let file_part = reqwest::multipart::Part::bytes(format!("data for {}", name).into_bytes())
                .file_name(format!("{}.csv", name));
            let form = reqwest::multipart::Form::new()
                .part("file", file_part)
                .text("project", format!("{}/{}", owner, slug))
                .text("name", name.to_string());
            let resp = s.client
                .post(format!("{}/data/upload", base))
                .header("Authorization", format!("Bearer {}", token))
                .multipart(form)
                .send()
                .await
                .unwrap();
            assert_eq!(resp.status(), 200, "Upload {} failed", name);
        }

        // 1. Create collection
        let resp = s.client
            .post(format!("{}/collections/{}/{}", base, owner, slug))
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .body(r#"{"name": "train-set"}"#)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200, "Create collection failed: {}", resp.text().await.unwrap_or_default());

        // Re-send to get parsed body
        let resp = s.client
            .post(format!("{}/collections/{}/{}", base, owner, slug))
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .body(r#"{"name": "test-set"}"#)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let create_body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(create_body["name"], "test-set");
        assert_eq!(create_body["yanked"], false);

        // 2. List collections
        let resp = s.client
            .get(format!("{}/collections/{}/{}", base, owner, slug))
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let list_body: Vec<serde_json::Value> = resp.json().await.unwrap();
        assert_eq!(list_body.len(), 2);

        // 3. Add members to train-set
        let resp = s.client
            .post(format!("{}/collections/{}/{}/train-set/add", base, owner, slug))
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .body(r#"{"members": [{"member_type": "data", "member_ref": "alpha"}, {"member_type": "data", "member_ref": "beta"}]}"#)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200, "Add members failed: {}", resp.text().await.unwrap_or_default());

        // Re-add to get body
        let resp = s.client
            .post(format!("{}/collections/{}/{}/train-set/add", base, owner, slug))
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .body(r#"{"members": [{"member_type": "data", "member_ref": "alpha"}]}"#)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let add_body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(add_body["version_number"], 2, "Should be version 2 (duplicate alpha skipped)");
        assert_eq!(add_body["members"].as_array().unwrap().len(), 2, "Should still have 2 members");

        // 4. Get collection detail
        let resp = s.client
            .get(format!("{}/collections/{}/{}/train-set", base, owner, slug))
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let detail: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(detail["name"], "train-set");
        assert_eq!(detail["version"]["version_number"], 2);
        assert_eq!(detail["version"]["members"].as_array().unwrap().len(), 2);

        // 5. Version log
        let resp = s.client
            .get(format!("{}/collections/{}/{}/train-set/log", base, owner, slug))
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let log_body: Vec<serde_json::Value> = resp.json().await.unwrap();
        assert_eq!(log_body.len(), 2, "Should have 2 versions");

        // 6. Flatten
        let resp = s.client
            .get(format!("{}/collections/{}/{}/train-set/flatten", base, owner, slug))
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let flat_body: Vec<serde_json::Value> = resp.json().await.unwrap();
        assert_eq!(flat_body.len(), 2, "Should have 2 leaf atoms");

        // 7. Remove a member
        let resp = s.client
            .post(format!("{}/collections/{}/{}/train-set/remove", base, owner, slug))
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .body(r#"{"refs": ["data:beta"]}"#)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let remove_body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(remove_body["version_number"], 3);
        assert_eq!(remove_body["members"].as_array().unwrap().len(), 1);

        // 8. Yank collection
        let resp = s.client
            .post(format!("{}/collections/{}/{}/test-set/yank", base, owner, slug))
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .body(r#"{"reason": "superseded"}"#)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
    });
}

/// Circular reference detection in collections.
#[test]
#[ignore] // Requires Docker
fn test_collection_cycle_detection() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        let (owner, slug, token) = setup_data_test(s, &format!("coll_cycle_{}", rand::random::<u32>())).await;
        let base = format!("{}/api/v1", s.base_url);

        // Create two collections
        for name in &["coll-a", "coll-b"] {
            let resp = s.client
                .post(format!("{}/collections/{}/{}", base, owner, slug))
                .header("Authorization", format!("Bearer {}", token))
                .header("Content-Type", "application/json")
                .body(format!(r#"{{"name": "{}"}}"#, name))
                .send()
                .await
                .unwrap();
            assert_eq!(resp.status(), 200, "Create {} failed", name);
        }

        // Add coll-b as member of coll-a (ok)
        let resp = s.client
            .post(format!("{}/collections/{}/{}/coll-a/add", base, owner, slug))
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .body(r#"{"members": [{"member_type": "collection", "member_ref": "coll-b"}]}"#)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        // Try to add coll-a as member of coll-b (would create cycle)
        let resp = s.client
            .post(format!("{}/collections/{}/{}/coll-b/add", base, owner, slug))
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .body(r#"{"members": [{"member_type": "collection", "member_ref": "coll-a"}]}"#)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 400, "Should reject circular reference");
        let body: serde_json::Value = resp.json().await.unwrap();
        assert!(body["message"].as_str().unwrap().contains("circular"), "Error should mention circular reference");

        // Self-reference should also be rejected
        let resp = s.client
            .post(format!("{}/collections/{}/{}/coll-a/add", base, owner, slug))
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .body(r#"{"members": [{"member_type": "collection", "member_ref": "coll-a"}]}"#)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 400, "Should reject self-reference");
    });
}
