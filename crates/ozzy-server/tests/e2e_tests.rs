//! End-to-end tests for the OzzyDB registry server.
//!
//! Tests the full stack including server-side transform execution (compute
//! pipeline), materialized cache, collaborator access control, and project
//! visibility.
//!
//! Requirements: Docker must be running. First run builds `ozzy-env:base`
//! image which pip-installs polars+pyarrow (~60s).
//!
//! Run: cargo test -p ozzy-server --test e2e_tests -- --ignored

use std::collections::BTreeMap;
use std::sync::Arc;

use arrow::array::{Int64Array, StringArray};
use arrow::datatypes::{DataType, Field, Schema as ArrowSchema};
use arrow::record_batch::RecordBatch;
use ozzy_core::canon::canonicalize_source;
use ozzy_core::commit::compute_commit_hash;
use ozzy_core::hash::{blake3_hash, transform_hash as compute_transform_hash};
use ozzy_core::project::{
    Commit, DataSource, Endpoint, PipelineEdge, PipelineNode, SourceType, Transform,
};
use ozzy_server::config::Config;
use ozzy_server::db::Database;
use ozzy_server::storage::ContentStorage;
use ozzy_server::{AppState, api};
use parquet::arrow::ArrowWriter;
use parquet::file::reader::FileReader;
use sqlx::postgres::PgPoolOptions;
use std::sync::LazyLock;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

// ========================================================================
// Test Infrastructure
// ========================================================================

struct TestServer {
    base_url: String,
    client: reqwest::Client,
    db: Database,
    _container: testcontainers::ContainerAsync<Postgres>,
    _storage_dir: tempfile::TempDir,
}

unsafe impl Send for TestServer {}
unsafe impl Sync for TestServer {}

static TEST_RT: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
});

/// Server with compute enabled (Docker transform execution).
static COMPUTE_SERVER: LazyLock<TestServer> = LazyLock::new(|| {
    TEST_RT.block_on(TestServer::start(true))
});

/// Server with compute disabled (tar fallback).
static NO_COMPUTE_SERVER: LazyLock<TestServer> = LazyLock::new(|| {
    TEST_RT.block_on(TestServer::start(false))
});

impl TestServer {
    async fn start(compute_enabled: bool) -> Self {
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
            max_tar_size_bytes: 1_073_741_824,
            max_upload_size_bytes: 104_857_600,
            cors_origins: "*".to_string(),
            compute: ozzy_server::config::ComputeConfig {
                enabled: compute_enabled,
                docker_runtime: "runc".to_string(),
                memory_limit: "4g".to_string(),
                cpu_limit: "2".to_string(),
                timeout_secs: 300,
                tmpfs_size: "1g".to_string(),
            },
            allowed_logins: vec![],
        };

        let storage =
            ContentStorage::from_config(&config).expect("Failed to create content storage");
        let materialized_storage =
            ContentStorage::from_config_with_prefix(&config, "materialized")
                .expect("Failed to create materialized storage");

        let state = AppState {
            config: Arc::new(config),
            db: db.clone(),
            storage,
            materialized_storage,
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

        TestServer {
            base_url,
            client: reqwest::Client::new(),
            db,
            _container: container,
            _storage_dir: storage_dir,
        }
    }

    async fn create_test_user(&self, suffix: &str) -> (String, String) {
        let github_id = rand::random::<i64>().abs();
        let username = format!("e2e_{}", suffix);
        let user = self
            .db
            .upsert_user_from_github(github_id, &username, None, None)
            .await
            .expect("Failed to create test user");

        let (plaintext, token_hash) = ozzy_server::auth::tokens::generate_api_token();
        let prefix = &plaintext[..12.min(plaintext.len())];

        self.db
            .create_token(
                user.id,
                &format!("test-token-{}", suffix),
                &token_hash,
                prefix,
                &["owner".to_string()],
                None,
            )
            .await
            .expect("Failed to create test token");

        (username, plaintext)
    }

    async fn push(
        &self,
        owner: &str,
        project: &str,
        token: &str,
        commit: &Commit,
        data_files: &[(&str, &[u8])],
        transform_files: &[(&str, &[u8])],
    ) -> reqwest::Response {
        let mut commit_json = serde_json::to_value(commit).unwrap();
        commit_json["ref"] = serde_json::json!("refs/heads/main");

        let mut form = reqwest::multipart::Form::new()
            .text("commit", serde_json::to_string(&commit_json).unwrap());

        for (name, data) in data_files {
            form = form.part(
                format!("data/{}", name),
                reqwest::multipart::Part::bytes(data.to_vec()),
            );
        }

        for (name, data) in transform_files {
            form = form.part(
                format!("transforms/{}", name),
                reqwest::multipart::Part::bytes(data.to_vec()),
            );
        }

        self.client
            .post(format!(
                "{}/api/v1/{}/{}/push",
                self.base_url, owner, project
            ))
            .header("Authorization", format!("Bearer {}", token))
            .multipart(form)
            .send()
            .await
            .expect("Push request failed")
    }

    /// Fetch an endpoint. Returns the raw response.
    async fn fetch_endpoint(
        &self,
        owner: &str,
        project: &str,
        endpoint: &str,
        ref_name: &str,
        token: Option<&str>,
    ) -> reqwest::Response {
        let mut req = self.client.get(format!(
            "{}/api/v1/{}/{}/{}@{}",
            self.base_url, owner, project, endpoint, ref_name
        ));

        if let Some(t) = token {
            req = req.header("Authorization", format!("Bearer {}", t));
        }

        req.send().await.expect("Fetch request failed")
    }

    /// Pull project content as tar bytes.
    async fn pull_tar(
        &self,
        owner: &str,
        project: &str,
        token: &str,
    ) -> reqwest::Response {
        self.client
            .get(format!(
                "{}/api/v1/{}/{}/pull?ref=main",
                self.base_url, owner, project
            ))
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .expect("Pull request failed")
    }
}

// ========================================================================
// Test Helpers
// ========================================================================

fn create_test_parquet(rows: &[(i64, &str)]) -> Vec<u8> {
    let schema = Arc::new(ArrowSchema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("name", DataType::Utf8, true),
    ]));

    let ids: Vec<i64> = rows.iter().map(|(id, _)| *id).collect();
    let names: Vec<&str> = rows.iter().map(|(_, name)| *name).collect();

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(Int64Array::from(ids)),
            Arc::new(StringArray::from(names)),
        ],
    )
    .unwrap();

    let mut buf = Vec::new();
    let mut writer = ArrowWriter::try_new(&mut buf, schema, None).unwrap();
    writer.write(&batch).unwrap();
    writer.close().unwrap();
    buf
}

/// Transform source that's a real executable Python function (no decorator).
/// The server's generated run.py calls `transform.clean_fn(inputs, params)` directly.
const COMPUTE_TRANSFORM_SOURCE: &str = r#"import polars as pl

def clean_fn(inputs, params):
    return inputs["main"]
"#;

/// Build a commit with data + transform + endpoint.
///
/// The endpoint has one node ("clean") using transform "clean" with function "clean_fn",
/// fed by data source "raw". This is a complete pipeline that can be executed server-side.
fn build_commit_with_endpoint(
    parent_hashes: Vec<String>,
    message: &str,
    parquet_data: &[u8],
    transform_source: &str,
) -> Commit {
    let data_hash = blake3_hash(parquet_data);

    // Schema hash from parquet metadata
    let reader: parquet::file::reader::SerializedFileReader<bytes::Bytes> =
        parquet::file::reader::SerializedFileReader::new(
            bytes::Bytes::from(parquet_data.to_vec()),
        )
        .unwrap();
    let parquet_meta = reader.metadata();
    let schema_desc = parquet_meta.file_metadata().schema_descr();
    let mut fields = Vec::new();
    for col in schema_desc.columns() {
        fields.push(serde_json::json!({
            "name": col.name().to_string(),
            "type": format!("{:?}", col.physical_type()),
            "nullable": col.self_type().is_optional(),
        }));
    }
    let schema_json = serde_json::json!({ "fields": fields });
    let schema_hash = ozzy_core::canon::hash_json(&schema_json);

    // Transform hashes
    let canonical = canonicalize_source(transform_source);
    let source_hash = blake3_hash(canonical.as_bytes());
    let lockfile_hash = blake3_hash(b"");
    let function_name = "clean_fn".to_string();
    let runtime = "python".to_string();
    let params_schema = serde_json::json!({});
    let params_schema_hash = ozzy_core::canon::hash_json(&params_schema);
    let composite_hash = compute_transform_hash(
        &source_hash,
        &function_name,
        &lockfile_hash,
        &runtime,
        &params_schema_hash,
    );

    let mut data_sources = BTreeMap::new();
    data_sources.insert(
        "raw".to_string(),
        DataSource {
            name: "raw".to_string(),
            path: "data/raw.parquet".to_string(),
            hash: data_hash,
            schema_hash,
            row_count: Some(3),
            byte_size: Some(parquet_data.len() as u64),
        },
    );

    let mut transforms = BTreeMap::new();
    transforms.insert(
        "clean".to_string(),
        Transform {
            name: "clean".to_string(),
            source_path: "transforms/clean.py".to_string(),
            hash: composite_hash,
            source_hash,
            function_name: function_name.clone(),
            input_schema: None,
            output_schema: None,
            runtime,
            lockfile_hash,
            params_schema: serde_json::json!({}),
            reproducible: true,
        },
    );

    let mut endpoints = BTreeMap::new();
    endpoints.insert(
        "cleaned".to_string(),
        Endpoint {
            name: "cleaned".to_string(),
            description: None,
            nodes: vec![PipelineNode {
                node_name: "clean".to_string(),
                transform_name: "clean".to_string(),
                params: serde_json::json!({}),
            }],
            edges: vec![PipelineEdge {
                target_node: "clean".to_string(),
                input_name: "main".to_string(),
                source_type: SourceType::DataSource,
                source_ref: "raw".to_string(),
                external_owner: None,
                external_project: None,
                external_endpoint: None,
                external_commit_hash: None,
            }],
        },
    );

    let mut commit = Commit {
        hash: String::new(),
        parent_hashes,
        author: "test@example.com".to_string(),
        message: message.to_string(),
        timestamp: chrono::Utc::now(),
        data_sources,
        transforms,
        endpoints,
    };
    commit.hash = compute_commit_hash(&commit);
    commit
}

// ========================================================================
// Compute Pipeline Tests
// ========================================================================

/// Push a project with an endpoint, fetch it with compute enabled,
/// and verify the response is valid parquet data.
#[test]
#[ignore] // Requires Docker (compute pipeline)
fn test_fetch_endpoint_executes_and_returns_parquet() {
    let s = &*COMPUTE_SERVER;
    TEST_RT.block_on(async {
        let (user, token) = s.create_test_user("compute_fetch").await;
        let project = "compute-proj";

        let parquet_data = create_test_parquet(&[(1, "alice"), (2, "bob"), (3, "charlie")]);
        let commit = build_commit_with_endpoint(
            vec![],
            "Compute test",
            &parquet_data,
            COMPUTE_TRANSFORM_SOURCE,
        );

        // Push
        let resp = s
            .push(
                &user,
                project,
                &token,
                &commit,
                &[("raw.parquet", parquet_data.as_slice())],
                &[("clean.py", COMPUTE_TRANSFORM_SOURCE.as_bytes())],
            )
            .await;
        let status = resp.status();
        let body = resp.text().await.unwrap();
        assert_eq!(status, 200, "Push failed: {}", body);

        // Fetch endpoint — should execute transform and return parquet
        let resp = s
            .fetch_endpoint(&user, project, "cleaned", "main", Some(&token))
            .await;
        assert_eq!(resp.status(), 200, "Fetch endpoint failed");

        let content_type = resp
            .headers()
            .get("content-type")
            .map(|v| v.to_str().unwrap().to_string())
            .unwrap_or_default();
        assert_eq!(
            content_type, "application/octet-stream",
            "Expected octet-stream, got {}",
            content_type
        );

        // Verify the response is valid parquet
        let response_bytes = resp.bytes().await.unwrap();
        assert!(!response_bytes.is_empty(), "Response body is empty");

        let reader = parquet::file::reader::SerializedFileReader::new(response_bytes.clone())
            .expect("Response is not valid parquet");
        let metadata = reader.metadata();
        assert_eq!(
            metadata.file_metadata().num_rows(),
            3,
            "Expected 3 rows in output parquet"
        );
    });
}

/// Fetch an endpoint twice and verify the materialized cache is hit.
#[test]
#[ignore] // Requires Docker (compute pipeline)
fn test_fetch_endpoint_cache_hit() {
    let s = &*COMPUTE_SERVER;
    TEST_RT.block_on(async {
        let (user, token) = s.create_test_user("cache_hit").await;
        let project = "cache-proj";

        let parquet_data = create_test_parquet(&[(10, "x"), (20, "y")]);
        let commit = build_commit_with_endpoint(
            vec![],
            "Cache test",
            &parquet_data,
            COMPUTE_TRANSFORM_SOURCE,
        );

        let resp = s
            .push(
                &user,
                project,
                &token,
                &commit,
                &[("raw.parquet", parquet_data.as_slice())],
                &[("clean.py", COMPUTE_TRANSFORM_SOURCE.as_bytes())],
            )
            .await;
        assert_eq!(resp.status(), 200);

        // First fetch — cache miss, executes transform
        let resp1 = s
            .fetch_endpoint(&user, project, "cleaned", "main", Some(&token))
            .await;
        assert_eq!(resp1.status(), 200, "First fetch failed");
        let bytes1 = resp1.bytes().await.unwrap();

        // Second fetch — should be a cache hit
        let resp2 = s
            .fetch_endpoint(&user, project, "cleaned", "main", Some(&token))
            .await;
        assert_eq!(resp2.status(), 200, "Second fetch failed");
        let bytes2 = resp2.bytes().await.unwrap();

        // Both should return identical data
        assert_eq!(
            bytes1, bytes2,
            "Cache hit should return identical bytes"
        );

        // Check DB: access_count should be >= 2
        let rows = sqlx::query_as::<_, (i32,)>(
            "SELECT access_count FROM materialized_cache WHERE access_count >= 2",
        )
        .fetch_all(s.db.pool())
        .await
        .unwrap();
        assert!(
            !rows.is_empty(),
            "Expected at least one cache entry with access_count >= 2"
        );
    });
}

/// With compute disabled, fetch should return a tar archive (fallback behavior).
#[test]
#[ignore] // Requires Docker
fn test_fetch_endpoint_compute_disabled_returns_tar() {
    let s = &*NO_COMPUTE_SERVER;
    TEST_RT.block_on(async {
        let (user, token) = s.create_test_user("tar_fallback").await;
        let project = "tar-proj";

        let parquet_data = create_test_parquet(&[(1, "a"), (2, "b")]);
        let commit = build_commit_with_endpoint(
            vec![],
            "Tar test",
            &parquet_data,
            COMPUTE_TRANSFORM_SOURCE,
        );

        let resp = s
            .push(
                &user,
                project,
                &token,
                &commit,
                &[("raw.parquet", parquet_data.as_slice())],
                &[("clean.py", COMPUTE_TRANSFORM_SOURCE.as_bytes())],
            )
            .await;
        assert_eq!(resp.status(), 200);

        let resp = s
            .fetch_endpoint(&user, project, "cleaned", "main", Some(&token))
            .await;
        assert_eq!(resp.status(), 200);

        let content_type = resp
            .headers()
            .get("content-type")
            .map(|v| v.to_str().unwrap().to_string())
            .unwrap_or_default();
        assert_eq!(
            content_type, "application/x-tar",
            "Expected tar when compute disabled, got {}",
            content_type
        );

        // Verify tar contains expected files
        let tar_bytes = resp.bytes().await.unwrap();
        let mut archive = tar::Archive::new(tar_bytes.as_ref());
        let mut found_endpoint = false;
        let mut found_data = false;
        let mut found_transform = false;

        for entry in archive.entries().unwrap() {
            let entry = entry.unwrap();
            let path = entry.path().unwrap().to_string_lossy().to_string();
            if path == "endpoint.json" {
                found_endpoint = true;
            } else if path.starts_with("data/") && path.ends_with(".parquet") {
                found_data = true;
            } else if path.starts_with("transforms/") && path.ends_with(".py") {
                found_transform = true;
            }
        }

        assert!(found_endpoint, "endpoint.json not found in tar");
        assert!(found_data, "data/*.parquet not found in tar");
        assert!(found_transform, "transforms/*.py not found in tar");
    });
}

// ========================================================================
// Access Control Tests
// ========================================================================

/// Private project should require authentication to access.
#[test]
#[ignore] // Requires Docker
fn test_private_project_requires_auth() {
    let s = &*NO_COMPUTE_SERVER;
    TEST_RT.block_on(async {
        let (user, token) = s.create_test_user("priv_auth").await;
        let project = "private-proj";

        let parquet_data = create_test_parquet(&[(1, "secret")]);
        let commit = build_commit_with_endpoint(
            vec![],
            "Private test",
            &parquet_data,
            COMPUTE_TRANSFORM_SOURCE,
        );

        let resp = s
            .push(
                &user,
                project,
                &token,
                &commit,
                &[("raw.parquet", parquet_data.as_slice())],
                &[("clean.py", COMPUTE_TRANSFORM_SOURCE.as_bytes())],
            )
            .await;
        assert_eq!(resp.status(), 200);

        // Unauthenticated fetch should fail (401 or 403)
        let resp = s
            .fetch_endpoint(&user, project, "cleaned", "main", None)
            .await;
        let status = resp.status().as_u16();
        assert!(
            status == 401 || status == 403,
            "Unauthenticated fetch of private project should be 401/403, got {}",
            status
        );

        // Authenticated fetch should work
        let resp = s
            .fetch_endpoint(&user, project, "cleaned", "main", Some(&token))
            .await;
        assert_eq!(
            resp.status(),
            200,
            "Owner should be able to fetch private project"
        );
    });
}

/// User B should not be able to access User A's private project without being a collaborator.
/// After being added as a collaborator, User B should have access.
#[test]
#[ignore] // Requires Docker
fn test_collaborator_can_read_private_project() {
    let s = &*NO_COMPUTE_SERVER;
    TEST_RT.block_on(async {
        let (user_a, token_a) = s.create_test_user("collab_owner").await;
        let (user_b, token_b) = s.create_test_user("collab_reader").await;
        let project = "collab-read-proj";

        let parquet_data = create_test_parquet(&[(1, "shared")]);
        let commit = build_commit_with_endpoint(
            vec![],
            "Collab test",
            &parquet_data,
            COMPUTE_TRANSFORM_SOURCE,
        );

        // User A pushes (creates private project)
        let resp = s
            .push(
                &user_a,
                project,
                &token_a,
                &commit,
                &[("raw.parquet", parquet_data.as_slice())],
                &[("clean.py", COMPUTE_TRANSFORM_SOURCE.as_bytes())],
            )
            .await;
        assert_eq!(resp.status(), 200);

        // User B tries to pull — should fail
        let resp = s.pull_tar(&user_a, project, &token_b).await;
        let status = resp.status().as_u16();
        assert!(
            status == 403,
            "Non-collaborator should get 403, got {}",
            status
        );

        // Add User B as read collaborator via DB
        let db_project = s
            .db
            .get_project(&user_a, project)
            .await
            .unwrap()
            .expect("Project should exist");
        let db_user_b = s
            .db
            .get_user_by_username(&user_b)
            .await
            .unwrap()
            .expect("User B should exist");
        s.db
            .upsert_project_collaborator(db_project.id, db_user_b.id, "read")
            .await
            .unwrap();

        // User B should now be able to pull
        let resp = s.pull_tar(&user_a, project, &token_b).await;
        assert_eq!(
            resp.status(),
            200,
            "Collaborator with read access should be able to pull"
        );
    });
}

/// Write collaborator should be able to push to another user's project.
#[test]
#[ignore] // Requires Docker
fn test_collaborator_write_access() {
    let s = &*NO_COMPUTE_SERVER;
    TEST_RT.block_on(async {
        let (user_a, token_a) = s.create_test_user("write_owner").await;
        let (user_b, token_b) = s.create_test_user("write_collab").await;
        let project = "collab-write-proj";

        let parquet_data = create_test_parquet(&[(1, "v1")]);
        let commit1 = build_commit_with_endpoint(
            vec![],
            "First commit",
            &parquet_data,
            COMPUTE_TRANSFORM_SOURCE,
        );

        // User A pushes first commit
        let resp = s
            .push(
                &user_a,
                project,
                &token_a,
                &commit1,
                &[("raw.parquet", parquet_data.as_slice())],
                &[("clean.py", COMPUTE_TRANSFORM_SOURCE.as_bytes())],
            )
            .await;
        assert_eq!(resp.status(), 200);

        // User B tries to push — should fail
        let parquet_v2 = create_test_parquet(&[(2, "v2")]);
        let transform_v2 = "import polars as pl\n\ndef clean_fn(inputs, params):\n    return inputs[\"main\"].head(1)\n";
        let commit2 = build_commit_with_endpoint(
            vec![commit1.hash.clone()],
            "Second commit",
            &parquet_v2,
            transform_v2,
        );

        let resp = s
            .push(
                &user_a,
                project,
                &token_b,
                &commit2,
                &[("raw.parquet", parquet_v2.as_slice())],
                &[("clean.py", transform_v2.as_bytes())],
            )
            .await;
        let status = resp.status().as_u16();
        assert!(
            status == 403,
            "Non-collaborator push should be 403, got {}",
            status
        );

        // Add User B as write collaborator
        let db_project = s
            .db
            .get_project(&user_a, project)
            .await
            .unwrap()
            .expect("Project should exist");
        let db_user_b = s
            .db
            .get_user_by_username(&user_b)
            .await
            .unwrap()
            .expect("User B should exist");
        s.db
            .upsert_project_collaborator(db_project.id, db_user_b.id, "write")
            .await
            .unwrap();

        // User B should now be able to push
        let resp = s
            .push(
                &user_a,
                project,
                &token_b,
                &commit2,
                &[("raw.parquet", parquet_v2.as_slice())],
                &[("clean.py", transform_v2.as_bytes())],
            )
            .await;
        let status = resp.status();
        let body = resp.text().await.unwrap();
        assert_eq!(
            status, 200,
            "Write collaborator should be able to push: {}",
            body
        );
    });
}

/// Public projects should be accessible without authentication.
#[test]
#[ignore] // Requires Docker
fn test_public_project_accessible_without_auth() {
    let s = &*NO_COMPUTE_SERVER;
    TEST_RT.block_on(async {
        let (user, token) = s.create_test_user("public_owner").await;
        let project = "public-proj";

        // Create a public project via the API
        let resp = s
            .client
            .post(format!("{}/api/v1/projects", s.base_url))
            .header("Authorization", format!("Bearer {}", token))
            .json(&serde_json::json!({
                "slug": project,
                "visibility": "public"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            200,
            "Create public project failed: {}",
            resp.text().await.unwrap()
        );

        // Push data to it
        let parquet_data = create_test_parquet(&[(1, "open")]);
        let commit = build_commit_with_endpoint(
            vec![],
            "Public commit",
            &parquet_data,
            COMPUTE_TRANSFORM_SOURCE,
        );

        let resp = s
            .push(
                &user,
                project,
                &token,
                &commit,
                &[("raw.parquet", parquet_data.as_slice())],
                &[("clean.py", COMPUTE_TRANSFORM_SOURCE.as_bytes())],
            )
            .await;
        assert_eq!(resp.status(), 200);

        // Unauthenticated fetch should work for public projects
        let resp = s
            .fetch_endpoint(&user, project, "cleaned", "main", None)
            .await;
        assert_eq!(
            resp.status(),
            200,
            "Public project should be accessible without auth"
        );
    });
}
