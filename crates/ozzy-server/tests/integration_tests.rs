//! Docker-based integration tests for the OzzyDB registry server.
//!
//! These tests spin up a real PostgreSQL container using testcontainers-rs
//! and test the full HTTP API including push/pull, concurrency, and auth.
//!
//! Requirements: Docker must be running.
//!
//! Run: cargo test -p ozzy-server --test integration_tests -- --ignored

use std::collections::BTreeMap;
use std::sync::Arc;

use arrow::array::{Int64Array, StringArray};
use arrow::datatypes::{DataType, Field, Schema as ArrowSchema};
use arrow::record_batch::RecordBatch;
use ozzy_core::canon::canonicalize_source;
use ozzy_core::commit::compute_commit_hash;
use ozzy_core::hash::{blake3_hash, transform_hash as compute_transform_hash};
use ozzy_core::project::{Commit, DataSource, Transform};
use ozzy_core::registry::protocol::PushResponse;
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
/// Using a shared runtime avoids "Tokio is shutting down" errors that occur
/// when #[tokio::test] creates per-test runtimes that outlive the OnceCell.
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
        // Initialize tracing to see server-side errors
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

        // Connect and run migrations
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

        // Create temp dir for content storage (local-only, no R2)
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
                enabled: false,
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

        let client = reqwest::Client::new();

        TestServer {
            base_url,
            client,
            db,
            _container: container,
            _storage_dir: storage_dir,
        }
    }

    /// Create a test user with an API token. Returns (username, bearer_token).
    async fn create_test_user(&self, suffix: &str) -> (String, String) {
        let github_id = rand::random::<i64>().abs();
        let username = format!("testuser_{}", suffix);
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

    /// Push a commit via the HTTP API.
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

    /// Get pull manifest (metadata only).
    async fn pull_manifest(
        &self,
        owner: &str,
        project: &str,
        token: &str,
    ) -> reqwest::Response {
        self.client
            .get(format!(
                "{}/api/v1/{}/{}/pull/manifest?ref=main",
                self.base_url, owner, project
            ))
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .expect("Pull manifest request failed")
    }

    /// Create a test user with specific token scopes. Returns (username, bearer_token).
    async fn create_test_user_with_scopes(
        &self,
        suffix: &str,
        scopes: Vec<String>,
    ) -> (String, String) {
        let github_id = rand::random::<i64>().abs();
        let username = format!("testuser_{}", suffix);
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
                &scopes,
                None,
            )
            .await
            .expect("Failed to create test token");

        (username, plaintext)
    }
}

/// Create a minimal valid parquet file.
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

/// Build a valid commit with properly computed hash.
fn build_commit(
    parent_hashes: Vec<String>,
    message: &str,
    parquet_data: &[u8],
    transform_source: &str,
    data_name: &str,
    transform_name: &str,
) -> Commit {
    let data_hash = blake3_hash(parquet_data);

    // Compute schema hash from parquet metadata
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

    // Canonicalize transform for hash
    let canonical = canonicalize_source(transform_source);
    let source_hash = blake3_hash(canonical.as_bytes());
    let lockfile_hash = blake3_hash(b"");
    let function_name = format!("{}_fn", transform_name);
    let runtime = "python".to_string();
    let params_schema = serde_json::json!({});
    let params_schema_hash = ozzy_core::canon::hash_json(&params_schema);
    // Composite hash: blake3(source_hash + function_name + lockfile_hash + runtime + params_schema_hash)
    let composite_hash = compute_transform_hash(
        &source_hash,
        &function_name,
        &lockfile_hash,
        &runtime,
        &params_schema_hash,
    );

    let mut data_sources = BTreeMap::new();
    data_sources.insert(
        data_name.to_string(),
        DataSource {
            name: data_name.to_string(),
            path: format!("data/{}.parquet", data_name),
            hash: data_hash,
            schema_hash,
            row_count: Some(3),
            byte_size: Some(parquet_data.len() as u64),
        },
    );

    let mut transforms = BTreeMap::new();
    transforms.insert(
        transform_name.to_string(),
        Transform {
            name: transform_name.to_string(),
            source_path: format!("transforms/{}.py", transform_name),
            hash: composite_hash,
            source_hash,
            function_name,
            input_schema: None,
            output_schema: None,
            runtime,
            lockfile_hash,
            params_schema: serde_json::json!({}),
            reproducible: true,
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
        endpoints: BTreeMap::new(),
    };
    commit.hash = compute_commit_hash(&commit);
    commit
}

const TRANSFORM_SOURCE: &str = r#"import ozzy

@ozzy.transform()
def clean_fn(inputs, params):
    return inputs["main"]
"#;

// ========================================================================
// Tests
// ========================================================================

/// Push a commit then pull it back, verifying the tar contains the correct files
/// and the commit hash matches.
#[test]
#[ignore] // Requires Docker
fn test_push_pull_roundtrip() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        let (user, token) = s.create_test_user("roundtrip").await;
        let project = "roundtrip-proj";

        let parquet_data = create_test_parquet(&[(1, "alice"), (2, "bob"), (3, "charlie")]);
        let commit = build_commit(
            vec![],
            "Initial commit",
            &parquet_data,
            TRANSFORM_SOURCE,
            "raw",
            "clean",
        );
        let original_hash = commit.hash.clone();

        // Push
        let resp = s
            .push(
                &user,
                project,
                &token,
                &commit,
                &[("raw.parquet", parquet_data.as_slice())],
                &[("clean.py", TRANSFORM_SOURCE.as_bytes())],
            )
            .await;
        assert_eq!(resp.status(), 200, "Push failed: {}", resp.text().await.unwrap());

        // Pull as tar
        let resp = s.pull_tar(&user, project, &token).await;
        assert_eq!(resp.status(), 200, "Pull failed");
        let tar_bytes = resp.bytes().await.unwrap();

        // Parse tar and verify contents
        let mut archive = tar::Archive::new(tar_bytes.as_ref());
        let mut found_commit = false;
        let mut found_data = false;
        let mut found_transform = false;

        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let path = entry.path().unwrap().to_string_lossy().to_string();

            if path == "commit.json" {
                let mut buf = Vec::new();
                std::io::Read::read_to_end(&mut entry, &mut buf).unwrap();
                let commit_json: serde_json::Value = serde_json::from_slice(&buf).unwrap();
                assert_eq!(commit_json["hash"], original_hash);
                assert_eq!(commit_json["message"], "Initial commit");
                found_commit = true;
            } else if path == "data/raw.parquet" {
                let mut buf = Vec::new();
                std::io::Read::read_to_end(&mut entry, &mut buf).unwrap();
                let pulled_hash = blake3_hash(&buf);
                assert_eq!(pulled_hash, commit.data_sources["raw"].hash);
                found_data = true;
            } else if path == "transforms/clean.py" {
                let mut buf = Vec::new();
                std::io::Read::read_to_end(&mut entry, &mut buf).unwrap();
                let text = String::from_utf8(buf).unwrap();
                let canonical = canonicalize_source(&text);
                let pulled_hash = blake3_hash(canonical.as_bytes());
                assert_eq!(pulled_hash, commit.transforms["clean"].source_hash);
                found_transform = true;
            }
        }

        assert!(found_commit, "commit.json not found in tar");
        assert!(found_data, "data/raw.parquet not found in tar");
        assert!(found_transform, "transforms/clean.py not found in tar");
    });
}

/// Two users push different commits to separate projects concurrently.
/// Both should succeed without interference.
#[test]
#[ignore] // Requires Docker
fn test_concurrent_pushes_different_users() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        let (user_a, token_a) = s.create_test_user("conc_a").await;
        let (user_b, token_b) = s.create_test_user("conc_b").await;

        let parquet_a = create_test_parquet(&[(1, "alpha"), (2, "beta")]);
        let parquet_b = create_test_parquet(&[(10, "gamma"), (20, "delta")]);

        let transform_a = "import ozzy\n\n@ozzy.transform()\ndef t_a(inputs, params):\n    return inputs[\"main\"]\n";
        let transform_b = "import ozzy\n\n@ozzy.transform()\ndef t_b(inputs, params):\n    return inputs[\"main\"]\n";

        let commit_a = build_commit(vec![], "Commit A", &parquet_a, transform_a, "raw", "proc_a");
        let commit_b = build_commit(vec![], "Commit B", &parquet_b, transform_b, "raw", "proc_b");

        let data_a: Vec<(&str, &[u8])> = vec![("raw.parquet", parquet_a.as_slice())];
        let transforms_a: Vec<(&str, &[u8])> = vec![("proc_a.py", transform_a.as_bytes())];
        let data_b: Vec<(&str, &[u8])> = vec![("raw.parquet", parquet_b.as_slice())];
        let transforms_b: Vec<(&str, &[u8])> = vec![("proc_b.py", transform_b.as_bytes())];

        let (resp_a, resp_b) = tokio::join!(
            s.push(&user_a, "conc-proj-a", &token_a, &commit_a, &data_a, &transforms_a),
            s.push(&user_b, "conc-proj-b", &token_b, &commit_b, &data_b, &transforms_b)
        );

        let status_a = resp_a.status();
        let body_a = resp_a.text().await.unwrap();
        assert_eq!(status_a, 200, "Push A failed: {}", body_a);

        let status_b = resp_b.status();
        let body_b = resp_b.text().await.unwrap();
        assert_eq!(status_b, 200, "Push B failed: {}", body_b);

        let result_a: PushResponse = serde_json::from_str(&body_a).unwrap();
        let result_b: PushResponse = serde_json::from_str(&body_b).unwrap();

        assert_eq!(result_a.commit_hash, commit_a.hash);
        assert_eq!(result_b.commit_hash, commit_b.hash);
    });
}

/// Two pushes to the same project with identical data files.
/// Content deduplication should mean the blob is stored once.
/// Both pushes should succeed.
#[test]
#[ignore] // Requires Docker
fn test_content_deduplication_across_pushes() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        let (user, token) = s.create_test_user("dedup").await;
        let project = "dedup-proj";

        let shared_data = create_test_parquet(&[(1, "shared"), (2, "data")]);

        let transform_v1 = "import ozzy\n\n@ozzy.transform()\ndef v1_fn(inputs, params):\n    return inputs[\"main\"]\n";
        let transform_v2 = "import ozzy\n\n@ozzy.transform()\ndef v2_fn(inputs, params):\n    return inputs[\"main\"].head(1)\n";

        let commit1 = build_commit(vec![], "Version 1", &shared_data, transform_v1, "raw", "proc");

        let resp = s
            .push(
                &user,
                project,
                &token,
                &commit1,
                &[("raw.parquet", shared_data.as_slice())],
                &[("proc.py", transform_v1.as_bytes())],
            )
            .await;
        let status = resp.status();
        let body = resp.text().await.unwrap();
        assert_eq!(status, 200, "Push 1 failed: {}", body);

        let result1: PushResponse = serde_json::from_str(&body).unwrap();
        assert_eq!(result1.new_data_count, 1, "First push should upload data");

        let commit2 = build_commit(
            vec![commit1.hash.clone()],
            "Version 2",
            &shared_data,
            transform_v2,
            "raw",
            "proc",
        );

        let resp = s
            .push(
                &user,
                project,
                &token,
                &commit2,
                &[("raw.parquet", shared_data.as_slice())],
                &[("proc.py", transform_v2.as_bytes())],
            )
            .await;
        let status = resp.status();
        let body = resp.text().await.unwrap();
        assert_eq!(status, 200, "Push 2 failed: {}", body);

        let result2: PushResponse = serde_json::from_str(&body).unwrap();
        assert_eq!(
            result2.new_data_count, 0,
            "Second push should deduplicate data (got {} new)",
            result2.new_data_count
        );
        assert_eq!(
            result2.new_transform_count, 1,
            "Second push should upload new transform"
        );
    });
}

/// Concurrent cli-session token upserts for the same user.
/// After concurrent operations, exactly one cli-session token should exist.
#[test]
#[ignore] // Requires Docker
fn test_concurrent_token_upsert() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        let github_id = rand::random::<i64>().abs();
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
                let (plaintext, token_hash) = ozzy_server::auth::tokens::generate_api_token();
                let prefix = &plaintext[..12.min(plaintext.len())];

                let token_id = uuid::Uuid::new_v4();
                let expires = chrono::Utc::now() + chrono::Duration::days(90);
                sqlx::query(
                    r#"INSERT INTO api_tokens (id, user_id, name, token_hash, token_prefix, scopes, expires_at)
                       VALUES ($1, $2, $3, $4, $5, $6, $7)
                       ON CONFLICT (user_id, name) DO UPDATE SET
                           token_hash = EXCLUDED.token_hash,
                           token_prefix = EXCLUDED.token_prefix,
                           scopes = EXCLUDED.scopes,
                           expires_at = EXCLUDED.expires_at"#,
                )
                .bind(token_id)
                .bind(user_id)
                .bind("cli-session")
                .bind(&token_hash)
                .bind(prefix)
                .bind(&vec!["owner".to_string()])
                .bind(expires)
                .execute(db.pool())
                .await
                .unwrap();

                (i, plaintext)
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

/// Pull manifest should be consistent with the pushed commit.
#[test]
#[ignore] // Requires Docker
fn test_pull_manifest_consistency() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        let (user, token) = s.create_test_user("manifest").await;
        let project = "manifest-proj";

        let parquet_data = create_test_parquet(&[(1, "x"), (2, "y")]);
        let commit = build_commit(
            vec![],
            "Manifest test",
            &parquet_data,
            TRANSFORM_SOURCE,
            "raw",
            "clean",
        );

        let resp = s
            .push(
                &user,
                project,
                &token,
                &commit,
                &[("raw.parquet", parquet_data.as_slice())],
                &[("clean.py", TRANSFORM_SOURCE.as_bytes())],
            )
            .await;
        assert_eq!(resp.status(), 200);

        let resp = s.pull_manifest(&user, project, &token).await;
        assert_eq!(resp.status(), 200);

        let manifest: ozzy_core::registry::protocol::PullManifest =
            resp.json().await.unwrap();

        assert_eq!(manifest.commit_hash, commit.hash);
        assert_eq!(manifest.commit_message, "Manifest test");
        assert_eq!(
            manifest.data_hashes.get("raw"),
            Some(&commit.data_sources["raw"].hash),
        );
        // Manifest transform_hashes use source content hash (for content-addressed storage)
        assert_eq!(
            manifest.transform_hashes.get("clean"),
            Some(&commit.transforms["clean"].source_hash),
        );
        assert!(manifest.total_size_bytes > 0);
    });
}

/// Push then pull, and verify that the pulled commit.json can reproduce
/// the same commit hash (timestamp roundtrip test from R16).
#[test]
#[ignore] // Requires Docker
fn test_commit_hash_roundtrip() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        let (user, token) = s.create_test_user("hashrt").await;
        let project = "hashrt-proj";

        let parquet_data = create_test_parquet(&[(1, "foo"), (2, "bar")]);
        let commit = build_commit(
            vec![],
            "Hash roundtrip test",
            &parquet_data,
            TRANSFORM_SOURCE,
            "raw",
            "clean",
        );
        let original_hash = commit.hash.clone();

        let resp = s
            .push(
                &user,
                project,
                &token,
                &commit,
                &[("raw.parquet", parquet_data.as_slice())],
                &[("clean.py", TRANSFORM_SOURCE.as_bytes())],
            )
            .await;
        assert_eq!(resp.status(), 200);

        let resp = s.pull_tar(&user, project, &token).await;
        assert_eq!(resp.status(), 200);
        let tar_bytes = resp.bytes().await.unwrap();

        let mut archive = tar::Archive::new(tar_bytes.as_ref());
        let mut commit_json_bytes = None;

        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let path = entry.path().unwrap().to_string_lossy().to_string();
            if path == "commit.json" {
                let mut buf = Vec::new();
                std::io::Read::read_to_end(&mut entry, &mut buf).unwrap();
                commit_json_bytes = Some(buf);
            }
        }

        let commit_json_bytes = commit_json_bytes.expect("commit.json not found in tar");
        let pulled_commit: Commit =
            serde_json::from_slice(&commit_json_bytes).unwrap();

        let recomputed_hash = compute_commit_hash(&pulled_commit);
        assert_eq!(
            recomputed_hash, original_hash,
            "Commit hash mismatch after push/pull roundtrip: recomputed={}, original={}",
            recomputed_hash, original_hash
        );
    });
}

// ========================================================================
// Review Series 1 Wrapup Tests
// ========================================================================

/// Push with path traversal in data file names should be rejected with 400.
#[test]
#[ignore] // Requires Docker
fn test_push_rejects_path_traversal_filenames() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        let (user, token) = s.create_test_user("traversal").await;
        let project = "traversal-proj";

        let parquet_data = create_test_parquet(&[(1, "x")]);
        let commit = build_commit(
            vec![],
            "Traversal test",
            &parquet_data,
            TRANSFORM_SOURCE,
            "raw",
            "clean",
        );

        // Push with a path traversal filename: ../../../etc/passwd
        let mut commit_json = serde_json::to_value(&commit).unwrap();
        commit_json["ref"] = serde_json::json!("refs/heads/main");

        let form = reqwest::multipart::Form::new()
            .text("commit", serde_json::to_string(&commit_json).unwrap())
            .part(
                "data/../../../etc/passwd",
                reqwest::multipart::Part::bytes(parquet_data.to_vec()),
            );

        let resp = s.client
            .post(format!("{}/api/v1/{}/{}/push", s.base_url, user, project))
            .header("Authorization", format!("Bearer {}", token))
            .multipart(form)
            .send()
            .await
            .unwrap();

        assert_eq!(
            resp.status(),
            400,
            "Path traversal should be rejected with 400, got {}",
            resp.status()
        );
    });
}

/// Push without a lockfile should succeed (lockfile sentinel handling from R15).
#[test]
#[ignore] // Requires Docker
fn test_push_without_lockfile_succeeds() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        let (user, token) = s.create_test_user("nolockfile").await;
        let project = "nolockfile-proj";

        let parquet_data = create_test_parquet(&[(1, "a"), (2, "b")]);
        // Build commit with lockfile_hash = blake3(b"") sentinel
        let commit = build_commit(
            vec![],
            "No lockfile",
            &parquet_data,
            TRANSFORM_SOURCE,
            "raw",
            "clean",
        );

        // Verify the lockfile_hash is the sentinel
        assert_eq!(
            commit.transforms["clean"].lockfile_hash,
            blake3_hash(b""),
            "Test setup: transform should have empty lockfile sentinel"
        );

        let resp = s
            .push(
                &user,
                project,
                &token,
                &commit,
                &[("raw.parquet", parquet_data.as_slice())],
                &[("clean.py", TRANSFORM_SOURCE.as_bytes())],
            )
            .await;
        let status = resp.status();
        let body = resp.text().await.unwrap();
        assert_eq!(status, 200, "Push without lockfile should succeed: {}", body);
    });
}

/// Error responses should use the protocol struct format {error, message}.
#[test]
#[ignore] // Requires Docker
fn test_error_responses_use_consistent_format() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        // 401: No auth token on an endpoint that requires auth (push uses WriteAuthUser)
        let resp = s.client
            .post(format!("{}/api/v1/nobody/noproject/push", s.base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 401);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert!(body.get("error").is_some(), "401 response should have 'error' field: {:?}", body);
        assert!(body.get("message").is_some(), "401 response should have 'message' field: {:?}", body);

        // 404: Non-existent project with valid auth on pull (uses MaybeAuthUser)
        let (_user, token) = s.create_test_user("errformat").await;
        let resp = s.client
            .get(format!(
                "{}/api/v1/nobody/nonexistent/pull?ref=main",
                s.base_url
            ))
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .unwrap();
        let status = resp.status().as_u16();
        assert!(status == 403 || status == 404, "Expected 403/404, got {}", status);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert!(body.get("error").is_some(), "Error response should have 'error' field: {:?}", body);
        assert!(body.get("message").is_some(), "Error response should have 'message' field: {:?}", body);
    });
}

/// Project-scoped token should not be able to access account-wide endpoints.
#[test]
#[ignore] // Requires Docker
fn test_project_scoped_token_cannot_access_account_endpoints() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        // Create a user with a project-scoped token
        let (_user, scoped_token) = s
            .create_test_user_with_scopes(
                "scoped_auth",
                vec!["read:testuser_scoped_auth/myproject".to_string()],
            )
            .await;

        // GET /auth/me should be rejected for project-scoped tokens
        let resp = s.client
            .get(format!("{}/api/v1/auth/me", s.base_url))
            .header("Authorization", format!("Bearer {}", scoped_token))
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
            .header("Authorization", format!("Bearer {}", scoped_token))
            .send()
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            403,
            "Project-scoped token should not access /auth/token, got {}",
            resp.status()
        );

        // Verify that an unscoped token CAN access these
        let (_user2, owner_token) = s.create_test_user("owner_auth").await;
        let resp = s.client
            .get(format!("{}/api/v1/auth/me", s.base_url))
            .header("Authorization", format!("Bearer {}", owner_token))
            .send()
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            200,
            "Owner token should access /auth/me, got {}",
            resp.status()
        );
    });
}

/// Two pushes to the same project should advance the ref correctly.
#[test]
#[ignore] // Requires Docker
fn test_second_push_advances_ref() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        let (user, token) = s.create_test_user("refadv").await;
        let project = "refadv-proj";

        let parquet_v1 = create_test_parquet(&[(1, "first")]);
        let commit1 = build_commit(
            vec![],
            "First commit",
            &parquet_v1,
            TRANSFORM_SOURCE,
            "raw",
            "clean",
        );
        let hash1 = commit1.hash.clone();

        let resp = s
            .push(
                &user, project, &token, &commit1,
                &[("raw.parquet", parquet_v1.as_slice())],
                &[("clean.py", TRANSFORM_SOURCE.as_bytes())],
            )
            .await;
        assert_eq!(resp.status(), 200);

        // Second push with different data
        let parquet_v2 = create_test_parquet(&[(2, "second"), (3, "third")]);
        let transform_v2 = "import ozzy\n\n@ozzy.transform()\ndef clean_fn(inputs, params):\n    return inputs[\"main\"].head(1)\n";
        let commit2 = build_commit(
            vec![hash1.clone()],
            "Second commit",
            &parquet_v2,
            transform_v2,
            "raw",
            "clean",
        );
        let hash2 = commit2.hash.clone();

        let resp = s
            .push(
                &user, project, &token, &commit2,
                &[("raw.parquet", parquet_v2.as_slice())],
                &[("clean.py", transform_v2.as_bytes())],
            )
            .await;
        assert_eq!(resp.status(), 200);

        // Pull manifest should show the second commit
        let resp = s.pull_manifest(&user, project, &token).await;
        assert_eq!(resp.status(), 200);
        let manifest: ozzy_core::registry::protocol::PullManifest =
            resp.json().await.unwrap();
        assert_eq!(
            manifest.commit_hash, hash2,
            "Manifest should show second commit, not first"
        );
        assert_ne!(hash1, hash2, "Commits should have different hashes");
    });
}

/// Push with tags and verify they're stored correctly.
#[test]
#[ignore] // Requires Docker
fn test_push_with_tags() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        let (user, token) = s.create_test_user("tagpush").await;
        let project = "tagpush-proj";

        let parquet_data = create_test_parquet(&[(1, "tagged")]);
        let commit = build_commit(
            vec![],
            "Tagged commit",
            &parquet_data,
            TRANSFORM_SOURCE,
            "raw",
            "clean",
        );

        // Push with a tag (tags are objects mapping tag_name -> commit_hash)
        let mut commit_json = serde_json::to_value(&commit).unwrap();
        commit_json["ref"] = serde_json::json!("refs/heads/main");
        commit_json["tags"] = serde_json::json!({"v1.0.0": commit.hash});

        let form = reqwest::multipart::Form::new()
            .text("commit", serde_json::to_string(&commit_json).unwrap())
            .part(
                "data/raw.parquet",
                reqwest::multipart::Part::bytes(parquet_data.to_vec()),
            )
            .part(
                "transforms/clean.py",
                reqwest::multipart::Part::bytes(TRANSFORM_SOURCE.as_bytes().to_vec()),
            );

        let resp = s.client
            .post(format!("{}/api/v1/{}/{}/push", s.base_url, user, project))
            .header("Authorization", format!("Bearer {}", token))
            .multipart(form)
            .send()
            .await
            .unwrap();

        let status = resp.status();
        let body = resp.text().await.unwrap();
        assert_eq!(status, 200, "Push with tag should succeed: {}", body);

        // Pull by tag ref should return the same commit
        let resp = s.client
            .get(format!(
                "{}/api/v1/{}/{}/pull/manifest?ref=v1.0.0",
                s.base_url, user, project
            ))
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200, "Pull by tag should succeed");
        let manifest: ozzy_core::registry::protocol::PullManifest =
            resp.json().await.unwrap();
        assert_eq!(manifest.commit_hash, commit.hash, "Tag should point to the pushed commit");
    });
}
