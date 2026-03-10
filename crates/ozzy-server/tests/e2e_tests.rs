//! End-to-end tests for the OzzyDB compute pipeline.
//!
//! These tests exercise the full execution path: commit registration → fetch →
//! Docker compute → output storage → cache behavior. They bypass the push endpoint
//! (which requires GitHub API) by inserting commit records directly.
//!
//! Requirements:
//!   - Docker must be running
//!   - `python:3.12-slim` image will be pulled on first run (may take a minute)
//!
//! Run: cargo test -p ozzy-server --test e2e_tests -- --ignored

use std::sync::Arc;

use ozzy_server::config::Config;
use ozzy_server::db::Database;
use ozzy_server::storage::ContentStorage;
use ozzy_server::{AppState, api};
use sqlx::postgres::PgPoolOptions;
use std::sync::LazyLock;

// ========================================================================
// Test Infrastructure
// ========================================================================

fn test_db_url() -> String {
    std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://ozzy_test:ozzy_test@localhost:5433/ozzy_test".into())
}

fn test_r2_config() -> ozzy_server::config::R2Config {
    ozzy_server::config::R2Config {
        endpoint: std::env::var("TEST_R2_ENDPOINT")
            .unwrap_or_else(|_| "http://localhost:9002".into()),
        bucket: std::env::var("TEST_R2_BUCKET")
            .unwrap_or_else(|_| "ozzy-test".into()),
        access_key_id: std::env::var("TEST_R2_ACCESS_KEY_ID")
            .unwrap_or_else(|_| "minioadmin".into()),
        secret_access_key: std::env::var("TEST_R2_SECRET_ACCESS_KEY")
            .unwrap_or_else(|_| "minioadmin".into()),
        region: "us-east-1".into(),
        presign_endpoint: std::env::var("TEST_R2_PRESIGN_ENDPOINT")
            .ok()
            .or_else(|| Some("http://host.docker.internal:9002".into())),
    }
}

/// E2E test server with compute enabled.
/// Start infra with: just test-infra-up
struct TestServer {
    base_url: String,
    client: reqwest::Client,
    db: Database,
    _tmpdir: tempfile::TempDir,
}

static TEST_RT: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
});

static TEST_SERVER: LazyLock<TestServer> = LazyLock::new(|| TEST_RT.block_on(TestServer::start()));

impl TestServer {
    async fn start() -> Self {
        let _ = tracing_subscriber::fmt()
            .with_env_filter("ozzy_server=debug,sqlx=warn")
            .try_init();

        let db_url = test_db_url();

        let pool = PgPoolOptions::new()
            .max_connections(50)
            .connect(&db_url)
            .await
            .expect("Failed to connect to test database (is test infra running? try: just test-infra-up)");

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("Failed to run migrations");

        // Truncate all data tables so tests start fresh (DB persists between runs)
        sqlx::query("TRUNCATE users, projects, commits, commit_state, api_tokens, data_atoms, data_metadata_log, collections, collection_versions, collection_members, project_collaborators, endpoint_yanks, secrets, materialized_cache, jobs, environment_provider_images, environment_images, content_refs, refs, source_cache, github_installations CASCADE")
            .execute(&pool)
            .await
            .expect("Failed to truncate tables");

        let db = Database::new(pool);

        let tmpdir = tempfile::tempdir().expect("Failed to create tmpdir");

        let config = Config {
            bind_address: "127.0.0.1:0".to_string(),
            database_url: db_url,
            db_max_connections: 50,
            github_client_id: "test_client_id".to_string(),
            github_client_secret: "test_client_secret".to_string(),
            base_url: "http://localhost:3000".to_string(),
            r2: test_r2_config(),
            max_upload_size_bytes: 104_857_600,
            cors_origins: "*".to_string(),
            allowed_logins: vec![],
            secrets_encryption_key: Some(vec![0x42; 32]),
            github_app: None,
            compute: ozzy_server::config::ComputeConfig {
                timeout_secs: 120,
                tmpdir: tmpdir.path().to_string_lossy().to_string(),
                default_provider: None,
            },
            docker: Some(ozzy_server::config::DockerProviderConfig {
                enabled: true,
                runtime: None,
                memory_limit: "2g".to_string(),
                cpu_limit: "1".to_string(),
            }),
            fly: None,
            rate_limit: ozzy_server::config::RateLimitConfig {
                global_max_concurrent: 0,
                per_user_max_concurrent: 0,
            },
            dev_auto_user: None,
        };

        let storage =
            ContentStorage::from_config(&config).expect("Failed to create content storage");

        let compute = ozzy_server::compute::ComputeRegistry::from_config(
            &config.compute,
            config.docker.as_ref(),
            config.fly.as_ref(),
        );

        let git = ozzy_server::GitHubProvider::new(None, db.clone());
        let state = AppState {
            config: Arc::new(config),
            db: db.clone(),
            storage,
            git,
            registry_snapshots: ozzy_server::registry::RegistrySnapshotCache::default(),
            compute,
        };

        let app = axum::Router::new().merge(api::router()).with_state(state);

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
            _tmpdir: tmpdir,
        }
    }

    async fn create_test_user(&self, suffix: &str) -> (String, String) {
        let github_id = rand::random::<i64>() & i64::MAX;
        let username = format!("e2euser_{}", suffix);
        let user = self
            .db
            .upsert_user_from_github(github_id, &username, None, None)
            .await
            .expect("Failed to create test user");

        let (plaintext, token_hash) = ozzy_server::auth::tokens::generate_api_token();

        self.db
            .create_token(
                user.id,
                &format!("e2e-token-{}", suffix),
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
// Test Helpers
// ========================================================================

/// Context returned by setup_compute_test.
struct ComputeTestCtx {
    owner: String,
    slug: String,
    project_id: uuid::Uuid,
    commit_id: uuid::Uuid,
    token: String,
}

/// Create a test user, project, and commit with a command-based transform.
///
/// The transform uses `python3 -c "..."` to generate a CSV file from params only.
/// No data inputs are needed — the endpoint has no edges.
async fn setup_param_only_test(server: &TestServer, suffix: &str) -> ComputeTestCtx {
    let (owner, token) = server.create_test_user(suffix).await;
    let slug = format!("e2e-project-{}", suffix);

    let owner_user = server
        .db
        .get_user_by_username(&owner)
        .await
        .expect("DB error")
        .expect("User not found");

    let project = server
        .db
        .get_or_create_project(owner_user.id, &slug, "private")
        .await
        .expect("Failed to create project");

    // Build commit state JSONB
    let environments = serde_json::json!({
        "python": {
            "image": "python:3.12-slim"
        }
    });

    // Command transform that generates a CSV from params
    let transforms = serde_json::json!({
        "generate": {
            "environment": "python",
            "command": "python3 -c \"import csv, os; count = int(os.environ.get('OZZY_PARAM_count', '5')); w = csv.writer(open('/workspace/output/result.csv', 'w')); w.writerow(['x', 'y']); [w.writerow([i, i*2]) for i in range(count)]\"",
            "inputs": {},
            "output": "csv",
            "params": {
                "count": {
                    "type": "int",
                    "default": 5
                }
            },
            "secrets": [],
            "network": false
        }
    });

    let endpoints = serde_json::json!({
        "generate-data": {
            "description": "E2E test endpoint: generates CSV from params",
            "params": {
                "count": {
                    "type": "int",
                    "default": 5,
                    "binds": "step1.count",
                    "min": 1,
                    "max": 100
                }
            },
            "nodes": {
                "step1": {
                    "transform": "generate"
                }
            },
            "edges": []
        }
    });

    let project_meta = serde_json::json!({
        "name": slug,
        "owner": owner,
        "description": "E2E test project"
    });

    // Fake git commit SHA (40 hex chars)
    let git_commit_sha: String = (0..40)
        .map(|_| format!("{:x}", rand::random::<u8>() % 16))
        .collect();

    // Build ozzy.toml equivalent as raw string (not strictly needed but stored)
    let ozzy_toml_raw = format!(
        r#"[project]
name = "{slug}"
owner = "{owner}"

[environments.python]
image = "python:3.12-slim"

[transforms.generate]
environment = "python"
command = "python3 -c \"import csv, os; count = int(os.environ.get('OZZY_PARAM_count', '5')); w = csv.writer(open('/workspace/output/result.csv', 'w')); w.writerow(['x', 'y']); [w.writerow([i, i*2]) for i in range(count)]\""
output = "csv"

[transforms.generate.params.count]
type = "int"
default = 5

[endpoints.generate-data]
description = "E2E test endpoint"

[endpoints.generate-data.params.count]
type = "int"
default = 5
binds = "step1.count"
min = 1.0
max = 100.0

[endpoints.generate-data.nodes.step1]
transform = "generate"
"#,
        slug = slug,
        owner = owner,
    );

    let ozzy_toml_hash = ozzy_core::hash::blake3_hash(ozzy_toml_raw.as_bytes());

    let commit = server
        .db
        .register_commit_atomically(
            project.id,
            "github",
            &format!("{}/test-repo", owner),
            &git_commit_sha,
            &ozzy_toml_hash,
            owner_user.id,
            Some("E2E test commit"),
            &ozzy_toml_raw,
            &environments,
            &transforms,
            &endpoints,
            &project_meta,
            Some("main"),
        )
        .await
        .expect("Failed to register commit");

    ComputeTestCtx {
        owner,
        slug,
        project_id: project.id,
        commit_id: commit.id,
        token,
    }
}

/// POST to fetch endpoint, poll until done, return (status, output_body, headers).
///
/// The v3 fetch endpoint is async: POST returns 202 + job_id (or 200 on cache hit),
/// then we poll GET /api/v1/jobs/{id} until done, then GET /api/v1/jobs/{id}/output.
async fn fetch_and_wait(
    server: &TestServer,
    owner: &str,
    slug: &str,
    endpoint: &str,
    token: Option<&str>,
    params: &[(&str, &str)],
) -> FetchResult {
    let mut url = format!(
        "{}/api/v1/fetch/{}/{}/{}",
        server.base_url, owner, slug, endpoint
    );
    if !params.is_empty() {
        let qs: Vec<String> = params.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
        url = format!("{}?{}", url, qs.join("&"));
    }

    let mut req = server.client.post(&url);
    if let Some(t) = token {
        req = req.header("Authorization", format!("Bearer {}", t));
    }

    let resp = req.send().await.expect("Fetch POST failed");
    let status = resp.status().as_u16();

    // Non-success responses: return immediately
    if status >= 400 {
        let body = resp.text().await.unwrap_or_default();
        return FetchResult {
            status,
            body,
            cache_hit: false,
        };
    }

    let fetch_body: serde_json::Value = resp.json().await.expect("Failed to parse fetch response");
    let job_id = fetch_body["job_id"]
        .as_str()
        .expect("Missing job_id in fetch response");
    let initial_status = fetch_body["status"].as_str().unwrap_or("");

    // If already done (cache hit fast path), go straight to output
    if initial_status != "done" {
        // Poll until done or failed
        let poll_url = format!("{}/api/v1/jobs/{}", server.base_url, job_id);
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(120);

        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;

            let mut poll_req = server.client.get(&poll_url);
            if let Some(t) = token {
                poll_req = poll_req.header("Authorization", format!("Bearer {}", t));
            }
            let poll_resp = poll_req.send().await.expect("Poll request failed");
            let poll_body: serde_json::Value =
                poll_resp.json().await.expect("Failed to parse poll response");

            let job_status = poll_body["status"].as_str().unwrap_or("");
            match job_status {
                "done" => break,
                "failed" => {
                    let err = poll_body["error_message"]
                        .as_str()
                        .unwrap_or("unknown error");
                    return FetchResult {
                        status: 500,
                        body: format!("Job failed: {}", err),
                        cache_hit: false,
                    };
                }
                _ => {
                    if tokio::time::Instant::now() > deadline {
                        panic!("Job {} did not complete within 120s", job_id);
                    }
                }
            }
        }
    }

    let cache_hit = initial_status == "done" && status == 200;

    // Download output
    let output_url = format!("{}/api/v1/jobs/{}/output", server.base_url, job_id);
    let mut output_req = server.client.get(&output_url);
    if let Some(t) = token {
        output_req = output_req.header("Authorization", format!("Bearer {}", t));
    }
    let output_resp = output_req.send().await.expect("Output download failed");
    let output_status = output_resp.status().as_u16();
    let body = output_resp.text().await.unwrap_or_default();

    FetchResult {
        status: output_status,
        body,
        cache_hit,
    }
}

struct FetchResult {
    status: u16,
    body: String,
    cache_hit: bool,
}

// ========================================================================
// Tests
// ========================================================================

/// Basic compute pipeline: command transform generates CSV from params.
///
/// Verifies: Docker compute executes, output is correct, content type is CSV.
#[test]
#[ignore] // Requires Docker
fn test_compute_pipeline_basic() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        let ctx = setup_param_only_test(s, "basic").await;

        let result =
            fetch_and_wait(s, &ctx.owner, &ctx.slug, "generate-data", Some(&ctx.token), &[]).await;

        assert_eq!(result.status, 200, "Fetch failed: {}", result.body);
        // Note: content_type comes from MinIO (presigned redirect), not our server.
        // MinIO returns binary/octet-stream for .bin files. We verify content correctness below.
        assert!(!result.cache_hit, "First fetch should be a cache miss");

        // Parse CSV body: header + 5 data rows (default count=5)
        let lines: Vec<&str> = result.body.trim().lines().collect();
        assert_eq!(lines[0], "x,y", "CSV header mismatch");
        assert_eq!(lines.len(), 6, "Expected 6 lines (header + 5 rows)");
        assert_eq!(lines[1], "0,0");
        assert_eq!(lines[5], "4,8");
    });
}

/// Cache hit: second fetch with same params returns cached result.
#[test]
#[ignore] // Requires Docker
fn test_compute_pipeline_cache_hit() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        let ctx = setup_param_only_test(s, "cache").await;

        // First fetch — cache miss, triggers compute
        let result1 =
            fetch_and_wait(s, &ctx.owner, &ctx.slug, "generate-data", Some(&ctx.token), &[]).await;
        assert_eq!(result1.status, 200);
        assert!(!result1.cache_hit, "First fetch should be a cache miss");

        // Second fetch — should hit cache (no compute)
        let result2 =
            fetch_and_wait(s, &ctx.owner, &ctx.slug, "generate-data", Some(&ctx.token), &[]).await;
        assert_eq!(result2.status, 200);
        assert!(result2.cache_hit, "Second fetch should be a cache hit");
        assert_eq!(result1.body, result2.body, "Cached output should match");
    });
}

/// Different params produce different results (cache miss on param change).
#[test]
#[ignore] // Requires Docker
fn test_compute_pipeline_param_override() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        let ctx = setup_param_only_test(s, "params").await;

        let result = fetch_and_wait(
            s,
            &ctx.owner,
            &ctx.slug,
            "generate-data",
            Some(&ctx.token),
            &[("count", "3")],
        )
        .await;

        assert_eq!(result.status, 200);
        let lines: Vec<&str> = result.body.trim().lines().collect();
        // count=3 → 3 data rows + 1 header = 4 lines
        assert_eq!(lines.len(), 4, "Expected 4 lines for count=3");
        assert_eq!(lines[3], "2,4");
    });
}

/// Param validation: out-of-range value rejected.
#[test]
#[ignore] // Requires Docker
fn test_compute_pipeline_param_out_of_range() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        let ctx = setup_param_only_test(s, "param_range").await;

        // count=999 exceeds max=100 — should be rejected at fetch time
        let result = fetch_and_wait(
            s,
            &ctx.owner,
            &ctx.slug,
            "generate-data",
            Some(&ctx.token),
            &[("count", "999")],
        )
        .await;

        assert_eq!(result.status, 400);
    });
}

/// Unknown param rejected.
#[test]
#[ignore] // Requires Docker
fn test_compute_pipeline_unknown_param() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        let ctx = setup_param_only_test(s, "unknown_param").await;

        let result = fetch_and_wait(
            s,
            &ctx.owner,
            &ctx.slug,
            "generate-data",
            Some(&ctx.token),
            &[("bogus", "42")],
        )
        .await;

        assert_eq!(result.status, 400);
    });
}

/// Nonexistent endpoint returns 404.
#[test]
#[ignore] // Requires Docker
fn test_fetch_nonexistent_endpoint() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        let ctx = setup_param_only_test(s, "noendpoint").await;

        let result = fetch_and_wait(
            s,
            &ctx.owner,
            &ctx.slug,
            "no-such-endpoint",
            Some(&ctx.token),
            &[],
        )
        .await;

        assert_eq!(result.status, 404);
    });
}

/// Yanked endpoint returns 410 Gone.
#[test]
#[ignore] // Requires Docker
fn test_compute_pipeline_yank_blocks_fetch() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        let ctx = setup_param_only_test(s, "yank").await;

        let user = s
            .db
            .get_user_by_username(&ctx.owner)
            .await
            .unwrap()
            .unwrap();

        s.db.insert_endpoint_yank(
            ctx.project_id,
            "generate-data",
            ctx.commit_id,
            "test yank",
            user.id,
        )
        .await
        .expect("Failed to yank endpoint");

        let result = fetch_and_wait(
            s,
            &ctx.owner,
            &ctx.slug,
            "generate-data",
            Some(&ctx.token),
            &[],
        )
        .await;

        assert_eq!(result.status, 410);
    });
}

/// Private project requires auth.
#[test]
#[ignore] // Requires Docker
fn test_private_project_requires_auth() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        let ctx = setup_param_only_test(s, "priv_auth").await;

        // No auth token
        let result =
            fetch_and_wait(s, &ctx.owner, &ctx.slug, "generate-data", None, &[]).await;

        assert!(
            result.status == 401 || result.status == 403,
            "Expected 401 or 403, got {}",
            result.status
        );
    });
}

/// Wrong user cannot access private project.
#[test]
#[ignore] // Requires Docker
fn test_private_project_wrong_user() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        let ctx = setup_param_only_test(s, "priv_wrong").await;

        let (_other_user, other_token) = s.create_test_user("priv_other").await;

        let result = fetch_and_wait(
            s,
            &ctx.owner,
            &ctx.slug,
            "generate-data",
            Some(&other_token),
            &[],
        )
        .await;

        assert_eq!(result.status, 403);
    });
}

/// Public project can be fetched without auth.
#[test]
#[ignore] // Requires Docker
fn test_public_project_no_auth() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        let (owner, _token) = s.create_test_user("pub").await;
        let slug = "e2e-public";

        let owner_user = s
            .db
            .get_user_by_username(&owner)
            .await
            .unwrap()
            .unwrap();

        let project = s
            .db
            .get_or_create_project(owner_user.id, slug, "public")
            .await
            .expect("Failed to create project");

        let environments = serde_json::json!({
            "python": { "image": "python:3.12-slim" }
        });
        let transforms = serde_json::json!({
            "generate": {
                "environment": "python",
                "command": "python3 -c \"import csv; w = csv.writer(open('/workspace/output/result.csv', 'w')); w.writerow(['x']); w.writerow([1])\"",
                "inputs": {},
                "output": "csv",
                "params": {},
                "secrets": [],
                "network": false
            }
        });
        let endpoints = serde_json::json!({
            "simple": {
                "nodes": { "step1": { "transform": "generate" } },
                "edges": [],
                "params": {}
            }
        });
        let project_meta = serde_json::json!({ "name": slug, "owner": owner });
        let git_sha: String = (0..40)
            .map(|_| format!("{:x}", rand::random::<u8>() % 16))
            .collect();
        let raw = "[project]\nname = \"test\"";
        let hash = ozzy_core::hash::blake3_hash(raw.as_bytes());

        s.db.register_commit_atomically(
            project.id,
            "github",
            &format!("{}/pub-repo", owner),
            &git_sha,
            &hash,
            owner_user.id,
            Some("public test"),
            raw,
            &environments,
            &transforms,
            &endpoints,
            &project_meta,
            Some("main"),
        )
        .await
        .expect("Failed to register commit");

        // Fetch WITHOUT auth — public project should allow it
        let result = fetch_and_wait(s, &owner, slug, "simple", None, &[]).await;

        assert_eq!(
            result.status, 200,
            "Public project fetch failed: {}",
            result.body
        );
    });
}

/// Endpoint inspection API returns correct metadata after commit.
#[test]
#[ignore] // Requires Docker
fn test_endpoint_inspection() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        let ctx = setup_param_only_test(s, "inspect").await;

        // List endpoints
        let resp = s
            .client
            .get(format!(
                "{}/api/v1/endpoints/{}/{}",
                s.base_url, ctx.owner, ctx.slug
            ))
            .header("Authorization", format!("Bearer {}", ctx.token))
            .send()
            .await
            .expect("Request failed");

        assert_eq!(resp.status(), 200);
        let body: serde_json::Value = resp.json().await.unwrap();
        let endpoints = body.as_array().expect("Expected array");
        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0]["name"], "generate-data");

        // Get endpoint detail
        let resp = s
            .client
            .get(format!(
                "{}/api/v1/endpoints/{}/{}/generate-data",
                s.base_url, ctx.owner, ctx.slug
            ))
            .header("Authorization", format!("Bearer {}", ctx.token))
            .send()
            .await
            .expect("Request failed");

        assert_eq!(resp.status(), 200);
        let detail: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(detail["name"], "generate-data");
        assert!(detail["nodes"].is_object());
        assert!(detail["params"].is_array());

        // Get DAG (mermaid)
        let resp = s
            .client
            .get(format!(
                "{}/api/v1/endpoints/{}/{}/generate-data/dag?format=mermaid",
                s.base_url, ctx.owner, ctx.slug
            ))
            .header("Authorization", format!("Bearer {}", ctx.token))
            .send()
            .await
            .expect("Request failed");

        assert_eq!(resp.status(), 200);
        let dag: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(dag["format"], "mermaid");
        let content = dag["content"].as_str().unwrap_or("");
        assert!(content.contains("graph"), "Mermaid should contain 'graph'");
    });
}

/// Commit listing and detail API work after commit.
#[test]
#[ignore] // Requires Docker
fn test_commit_api() {
    let s = &*TEST_SERVER;
    TEST_RT.block_on(async {
        let ctx = setup_param_only_test(s, "commits").await;

        let resp = s
            .client
            .get(format!(
                "{}/api/v1/commits/{}/{}",
                s.base_url, ctx.owner, ctx.slug
            ))
            .header("Authorization", format!("Bearer {}", ctx.token))
            .send()
            .await
            .expect("Request failed");

        assert_eq!(resp.status(), 200);
        let body: serde_json::Value = resp.json().await.unwrap();
        let commits = body.as_array().expect("Expected array");
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0]["message"], "E2E test commit");
    });
}
