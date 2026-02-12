//! Database integration tests for v2 schema.
//!
//! These tests require a PostgreSQL database to run.
//! Set DATABASE_URL environment variable to run these tests.
//!
//! Example using Docker:
//! ```bash
//! docker run -p 5432:5432 -e POSTGRES_PASSWORD=test -e POSTGRES_DB=ozzy_test postgres:17-alpine
//! export DATABASE_URL=postgres://postgres:test@localhost/ozzy_test
//! cargo test --package ozzy-server db_tests
//! ```

use anyhow::Result;
use ozzy_server::db::Database;
use sqlx::postgres::PgPoolOptions;
use std::env;
use uuid::Uuid;

async fn get_test_db() -> Option<Database> {
    let url = env::var("DATABASE_URL").ok()?;

    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .ok()?;

    // Run migrations
    sqlx::migrate!("./migrations").run(&pool).await.ok()?;

    Some(Database::new(pool))
}

fn unique_name(prefix: &str) -> String {
    format!(
        "{}_{}",
        prefix,
        Uuid::new_v4().to_string().replace('-', "")[..8].to_string()
    )
}

// ========================================================================
// User Operations
// ========================================================================

#[tokio::test]
async fn test_upsert_and_get_user() -> Result<()> {
    let Some(db) = get_test_db().await else {
        eprintln!("Skipping: DATABASE_URL not set");
        return Ok(());
    };

    let github_id = (rand::random::<i64>() & i64::MAX);
    let login = unique_name("user");

    let user = db
        .upsert_user_from_github(
            github_id,
            &login,
            Some("test@example.com"),
            Some("https://avatar.url"),
        )
        .await?;

    assert_eq!(user.username, login);
    assert_eq!(user.github_id, Some(github_id));
    assert_eq!(user.email, Some("test@example.com".to_string()));

    // Get by github_id
    let found = db.get_user_by_github_id(github_id).await?.unwrap();
    assert_eq!(found.id, user.id);

    // Get by username
    let found = db.get_user_by_username(&login).await?.unwrap();
    assert_eq!(found.id, user.id);

    // Get by id
    let found = db.get_user_by_id(user.id).await?.unwrap();
    assert_eq!(found.username, login);

    // Upsert updates existing user
    let updated = db
        .upsert_user_from_github(github_id, &login, Some("new@example.com"), None)
        .await?;
    assert_eq!(updated.id, user.id);
    assert_eq!(updated.email, Some("new@example.com".to_string()));

    Ok(())
}

// ========================================================================
// Project Operations
// ========================================================================

#[tokio::test]
async fn test_create_and_get_project() -> Result<()> {
    let Some(db) = get_test_db().await else {
        return Ok(());
    };

    let user = db
        .upsert_user_from_github(
            (rand::random::<i64>() & i64::MAX),
            &unique_name("user"),
            None,
            None,
        )
        .await?;

    let slug = unique_name("proj");
    let project = db
        .create_project(user.id, &slug, Some("Test project"), "private")
        .await?;

    assert_eq!(project.slug, slug);
    assert_eq!(project.owner_id, user.id);
    assert_eq!(project.visibility, "private");

    // Get by owner + slug
    let found = db.get_project(&user.username, &slug).await?.unwrap();
    assert_eq!(found.id, project.id);

    // Get by id
    let found = db.get_project_by_id(project.id).await?.unwrap();
    assert_eq!(found.slug, slug);

    // List user projects
    let projects = db.list_user_projects(user.id).await?;
    assert!(projects.iter().any(|p| p.id == project.id));

    Ok(())
}

#[tokio::test]
async fn test_get_or_create_project() -> Result<()> {
    let Some(db) = get_test_db().await else {
        return Ok(());
    };

    let user = db
        .upsert_user_from_github(
            (rand::random::<i64>() & i64::MAX),
            &unique_name("user"),
            None,
            None,
        )
        .await?;

    let slug = unique_name("proj");

    // First call creates
    let p1 = db.get_or_create_project(user.id, &slug, "private").await?;
    // Second call returns same project
    let p2 = db.get_or_create_project(user.id, &slug, "private").await?;
    assert_eq!(p1.id, p2.id);

    Ok(())
}

// ========================================================================
// Collaborator Operations
// ========================================================================

#[tokio::test]
async fn test_collaborators() -> Result<()> {
    let Some(db) = get_test_db().await else {
        return Ok(());
    };

    let owner = db
        .upsert_user_from_github(
            (rand::random::<i64>() & i64::MAX),
            &unique_name("owner"),
            None,
            None,
        )
        .await?;
    let collab = db
        .upsert_user_from_github(
            (rand::random::<i64>() & i64::MAX),
            &unique_name("collab"),
            None,
            None,
        )
        .await?;

    let project = db
        .create_project(owner.id, &unique_name("proj"), None, "private")
        .await?;

    // Add collaborator
    let c = db
        .upsert_project_collaborator(project.id, collab.id, "read")
        .await?;
    assert_eq!(c.role, "read");

    // Update role
    let c = db
        .upsert_project_collaborator(project.id, collab.id, "write")
        .await?;
    assert_eq!(c.role, "write");

    // Get collaborator
    let fetched = db
        .get_project_collaborator(project.id, collab.id)
        .await?
        .unwrap();
    assert_eq!(fetched.role, "write");

    // List collaborators
    let collabs = db.list_project_collaborators(project.id).await?;
    assert_eq!(collabs.len(), 1);
    assert_eq!(collabs[0].username, collab.username);

    // Remove collaborator
    let removed = db
        .remove_project_collaborator(project.id, collab.id)
        .await?;
    assert!(removed);

    let collabs = db.list_project_collaborators(project.id).await?;
    assert!(collabs.is_empty());

    Ok(())
}

// ========================================================================
// API Token Operations
// ========================================================================

#[tokio::test]
async fn test_tokens() -> Result<()> {
    let Some(db) = get_test_db().await else {
        return Ok(());
    };

    let user = db
        .upsert_user_from_github(
            (rand::random::<i64>() & i64::MAX),
            &unique_name("user"),
            None,
            None,
        )
        .await?;

    let token_hash = format!("hash_{}", Uuid::new_v4());

    let token = db
        .create_token(user.id, "test-token", &token_hash, "account", None, None)
        .await?;
    assert_eq!(token.name, "test-token");
    assert_eq!(token.scope, "account");
    assert!(token.project_id.is_none());

    // Get by hash
    let found = db.get_token_by_hash(&token_hash).await?.unwrap();
    assert_eq!(found.id, token.id);

    // List tokens
    let tokens = db.list_user_tokens(user.id).await?;
    assert!(tokens.iter().any(|t| t.id == token.id));

    // Touch token
    db.touch_token(token.id).await?;
    let touched = db.get_token_by_hash(&token_hash).await?.unwrap();
    assert!(touched.last_used_at.is_some());

    // Delete by name
    let deleted = db.delete_token_by_name(user.id, "test-token").await?;
    assert!(deleted);
    assert!(db.get_token_by_hash(&token_hash).await?.is_none());

    Ok(())
}

#[tokio::test]
async fn test_project_scoped_token() -> Result<()> {
    let Some(db) = get_test_db().await else {
        return Ok(());
    };

    let user = db
        .upsert_user_from_github(
            (rand::random::<i64>() & i64::MAX),
            &unique_name("user"),
            None,
            None,
        )
        .await?;

    let project = db
        .create_project(user.id, &unique_name("proj"), None, "private")
        .await?;

    let scope = format!("project:{}/{}", user.username, project.slug);
    let token_hash = format!("hash_{}", Uuid::new_v4());

    let token = db
        .create_token(
            user.id,
            "proj-token",
            &token_hash,
            &scope,
            Some(project.id),
            None,
        )
        .await?;
    assert_eq!(token.scope, scope);
    assert_eq!(token.project_id, Some(project.id));

    Ok(())
}

#[tokio::test]
async fn test_session_token_upsert() -> Result<()> {
    let Some(db) = get_test_db().await else {
        return Ok(());
    };

    let user = db
        .upsert_user_from_github(
            (rand::random::<i64>() & i64::MAX),
            &unique_name("user"),
            None,
            None,
        )
        .await?;

    let expires = chrono::Utc::now() + chrono::Duration::days(90);

    let hash1 = format!("session_{}", Uuid::new_v4());
    let hash2 = format!("session_{}", Uuid::new_v4());

    // First upsert creates
    db.upsert_session_token(user.id, "cli-session", &hash1, expires)
        .await?;
    let tokens = db.list_user_tokens(user.id).await?;
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].token_hash, hash1);

    // Second upsert updates the same token
    db.upsert_session_token(user.id, "cli-session", &hash2, expires)
        .await?;
    let tokens = db.list_user_tokens(user.id).await?;
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].token_hash, hash2);

    Ok(())
}

// ========================================================================
// Commit Operations
// ========================================================================

#[tokio::test]
async fn test_commits() -> Result<()> {
    let Some(db) = get_test_db().await else {
        return Ok(());
    };

    let user = db
        .upsert_user_from_github(
            (rand::random::<i64>() & i64::MAX),
            &unique_name("user"),
            None,
            None,
        )
        .await?;
    let project = db
        .create_project(user.id, &unique_name("proj"), None, "private")
        .await?;

    let sha = format!("{:040x}", rand::random::<u128>());
    let commit = db
        .insert_commit(
            project.id,
            "github",
            &format!("{}/test-repo", user.username),
            &sha,
            "toml_hash_abc",
            user.id,
            Some("Initial commit"),
        )
        .await?;

    assert_eq!(commit.git_commit_sha, sha);
    assert_eq!(commit.git_provider, "github");
    assert_eq!(commit.message, Some("Initial commit".to_string()));

    // Get by SHA
    let found = db.get_commit_by_sha(project.id, &sha).await?.unwrap();
    assert_eq!(found.id, commit.id);

    // List commits
    let commits = db.list_commits(project.id, 10).await?;
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].id, commit.id);

    Ok(())
}

// ========================================================================
// Commit State Operations
// ========================================================================

#[tokio::test]
async fn test_commit_state() -> Result<()> {
    let Some(db) = get_test_db().await else {
        return Ok(());
    };

    let user = db
        .upsert_user_from_github(
            (rand::random::<i64>() & i64::MAX),
            &unique_name("user"),
            None,
            None,
        )
        .await?;
    let project = db
        .create_project(user.id, &unique_name("proj"), None, "private")
        .await?;
    let sha = format!("{:040x}", rand::random::<u128>());
    let commit = db
        .insert_commit(
            project.id,
            "github",
            "user/repo",
            &sha,
            "toml_hash",
            user.id,
            None,
        )
        .await?;

    let environments =
        serde_json::json!({"default": {"base": "ozzydb/python:3.12", "lockfile": "uv.lock"}});
    let transforms = serde_json::json!({"qc": {"source": "transforms/qc.py:quality_control"}});
    let endpoints = serde_json::json!({"analysis": {"description": "Main analysis"}});
    let project_meta = serde_json::json!({"name": "test", "owner": "user"});

    let state = db
        .insert_commit_state(
            commit.id,
            "[project]\nname = \"test\"",
            &environments,
            &transforms,
            &endpoints,
            &project_meta,
        )
        .await?;

    assert_eq!(state.commit_id, commit.id);
    assert_eq!(state.environments, environments);

    // Get commit state
    let found = db.get_commit_state(commit.id).await?.unwrap();
    assert_eq!(found.transforms, transforms);

    Ok(())
}

// ========================================================================
// Ref Operations
// ========================================================================

#[tokio::test]
async fn test_refs() -> Result<()> {
    let Some(db) = get_test_db().await else {
        return Ok(());
    };

    let user = db
        .upsert_user_from_github(
            (rand::random::<i64>() & i64::MAX),
            &unique_name("user"),
            None,
            None,
        )
        .await?;
    let project = db
        .create_project(user.id, &unique_name("proj"), None, "private")
        .await?;
    let sha = format!("{:040x}", rand::random::<u128>());
    let commit = db
        .insert_commit(
            project.id,
            "github",
            "user/repo",
            &sha,
            "hash",
            user.id,
            None,
        )
        .await?;

    // Create a branch ref
    let r = db
        .upsert_ref(project.id, "main", "branch", commit.id)
        .await?;
    assert_eq!(r.ref_name, "main");
    assert_eq!(r.ref_type, "branch");

    // Resolve ref
    let found = db.resolve_ref(project.id, "main").await?.unwrap();
    assert_eq!(found.commit_id, commit.id);

    // List refs
    let refs = db.list_refs(project.id).await?;
    assert_eq!(refs.len(), 1);

    // Upsert advances the ref (new commit)
    let sha2 = format!("{:040x}", rand::random::<u128>());
    let commit2 = db
        .insert_commit(
            project.id,
            "github",
            "user/repo",
            &sha2,
            "hash2",
            user.id,
            None,
        )
        .await?;
    let r2 = db
        .upsert_ref(project.id, "main", "branch", commit2.id)
        .await?;
    assert_eq!(r2.commit_id, commit2.id);

    // Delete ref
    let deleted = db.delete_ref(project.id, "main").await?;
    assert!(deleted);
    assert!(db.resolve_ref(project.id, "main").await?.is_none());

    Ok(())
}

// ========================================================================
// Data Atom Operations
// ========================================================================

#[tokio::test]
async fn test_data_atoms() -> Result<()> {
    let Some(db) = get_test_db().await else {
        return Ok(());
    };

    let user = db
        .upsert_user_from_github(
            (rand::random::<i64>() & i64::MAX),
            &unique_name("user"),
            None,
            None,
        )
        .await?;
    let project = db
        .create_project(user.id, &unique_name("proj"), None, "private")
        .await?;

    let name = unique_name("data");
    let hash = format!("{:064x}", rand::random::<u128>());

    let atom = db
        .insert_data_atom(
            project.id,
            &name,
            &hash,
            "application/vnd.apache.parquet",
            1024,
            &format!("data/{}", hash),
            user.id,
        )
        .await?;

    assert_eq!(atom.name, name);
    assert_eq!(atom.hash, hash);
    assert!(!atom.yanked);

    // Get by name
    let found = db.get_data_atom(project.id, &name).await?.unwrap();
    assert_eq!(found.id, atom.id);

    // List atoms
    let atoms = db.list_data_atoms(project.id).await?;
    assert!(atoms.iter().any(|a| a.id == atom.id));

    // Yank
    let yanked = db.yank_data_atom(project.id, &name, "Bad data").await?;
    assert!(yanked);
    let found = db.get_data_atom(project.id, &name).await?.unwrap();
    assert!(found.yanked);
    assert_eq!(found.yank_reason, Some("Bad data".to_string()));

    Ok(())
}

// ========================================================================
// Content Ref Operations
// ========================================================================

#[tokio::test]
async fn test_content_refs() -> Result<()> {
    let Some(db) = get_test_db().await else {
        return Ok(());
    };

    let hash = format!("{:064x}", rand::random::<u128>());

    // First upsert creates with ref_count=1
    let cr = db
        .upsert_content_ref(
            &hash,
            &format!("data/{}", hash),
            "application/vnd.apache.parquet",
            2048,
        )
        .await?;
    assert_eq!(cr.ref_count, 1);

    // Second upsert increments ref_count
    let cr = db
        .upsert_content_ref(
            &hash,
            &format!("data/{}", hash),
            "application/vnd.apache.parquet",
            2048,
        )
        .await?;
    assert_eq!(cr.ref_count, 2);

    // Get content ref
    let found = db.get_content_ref(&hash).await?.unwrap();
    assert_eq!(found.ref_count, 2);

    Ok(())
}

// ========================================================================
// Data Metadata Operations
// ========================================================================

#[tokio::test]
async fn test_metadata_log() -> Result<()> {
    let Some(db) = get_test_db().await else {
        return Ok(());
    };

    let user = db
        .upsert_user_from_github(
            (rand::random::<i64>() & i64::MAX),
            &unique_name("user"),
            None,
            None,
        )
        .await?;
    let project = db
        .create_project(user.id, &unique_name("proj"), None, "private")
        .await?;
    let hash = format!("{:064x}", rand::random::<u128>());
    let atom = db
        .insert_data_atom(
            project.id,
            &unique_name("data"),
            &hash,
            "parquet",
            100,
            &format!("data/{}", hash),
            user.id,
        )
        .await?;

    // Append metadata entries
    db.append_metadata(
        atom.id,
        "description",
        &serde_json::json!("First description"),
        user.id,
    )
    .await?;

    // Brief delay to ensure different timestamps
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    db.append_metadata(
        atom.id,
        "description",
        &serde_json::json!("Updated description"),
        user.id,
    )
    .await?;

    // Latest should be the second one
    let latest = db
        .get_latest_metadata(atom.id, "description")
        .await?
        .unwrap();
    assert_eq!(latest.value, serde_json::json!("Updated description"));

    // History should have both, newest first
    let history = db.get_metadata_history(atom.id, "description").await?;
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].value, serde_json::json!("Updated description"));
    assert_eq!(history[1].value, serde_json::json!("First description"));

    Ok(())
}

// ========================================================================
// GitHub Installation Operations
// ========================================================================

#[tokio::test]
async fn test_github_installations() -> Result<()> {
    let Some(db) = get_test_db().await else {
        return Ok(());
    };

    let install_id = (rand::random::<i64>() & i64::MAX);
    let login = unique_name("org");

    let inst = db
        .upsert_github_installation(install_id, "Organization", &login)
        .await?;
    assert_eq!(inst.installation_id, install_id);
    assert_eq!(inst.account_login, login);

    // Get by login
    let found = db.get_github_installation_by_login(&login).await?.unwrap();
    assert_eq!(found.installation_id, install_id);

    // Upsert updates
    let updated = db
        .upsert_github_installation(install_id, "User", &login)
        .await?;
    assert_eq!(updated.account_type, "User");

    // Delete
    let deleted = db.delete_github_installation(install_id).await?;
    assert!(deleted);
    assert!(db.get_github_installation_by_login(&login).await?.is_none());

    Ok(())
}

// ========================================================================
// Project Update/Delete Operations
// ========================================================================

#[tokio::test]
async fn test_update_and_delete_project() -> Result<()> {
    let Some(db) = get_test_db().await else {
        return Ok(());
    };

    let user = db
        .upsert_user_from_github(
            (rand::random::<i64>() & i64::MAX),
            &unique_name("user"),
            None,
            None,
        )
        .await?;
    let project = db
        .create_project(user.id, &unique_name("proj"), Some("old desc"), "private")
        .await?;

    // Update
    let updated = db
        .update_project(project.id, Some("new desc"), "public")
        .await?
        .unwrap();
    assert_eq!(updated.description, Some("new desc".to_string()));
    assert_eq!(updated.visibility, "public");

    // Delete
    let deleted = db.delete_project(project.id).await?;
    assert!(deleted);
    assert!(db.get_project_by_id(project.id).await?.is_none());

    Ok(())
}

// ========================================================================
// Collection Operations
// ========================================================================

#[tokio::test]
async fn test_collections() -> Result<()> {
    let Some(db) = get_test_db().await else {
        return Ok(());
    };

    let user = db
        .upsert_user_from_github(
            (rand::random::<i64>() & i64::MAX),
            &unique_name("user"),
            None,
            None,
        )
        .await?;
    let project = db
        .create_project(user.id, &unique_name("proj"), None, "private")
        .await?;

    let name = unique_name("coll");
    let coll = db.create_collection(project.id, &name, user.id).await?;
    assert_eq!(coll.name, name);
    assert!(!coll.yanked);

    // Get
    let found = db.get_collection(project.id, &name).await?.unwrap();
    assert_eq!(found.id, coll.id);

    // List
    let colls = db.list_collections(project.id).await?;
    assert!(colls.iter().any(|c| c.id == coll.id));

    // Create version with members (atomically in transaction)
    let (ver, members) = db
        .create_collection_version_with_members(
            coll.id,
            "version_hash_1",
            user.id,
            &[
                ("data".into(), "my_data".into(), "hash_aaa".into(), 0),
                ("collection".into(), "sub_coll".into(), "hash_bbb".into(), 1),
            ],
        )
        .await?;
    assert_eq!(ver.version_number, 1);
    assert_eq!(ver.hash, "version_hash_1");
    assert_eq!(members.len(), 2);
    assert_eq!(members[0].member_type, "data");
    assert_eq!(members[1].member_type, "collection");

    // Get members
    let fetched_members = db.get_collection_members(ver.id).await?;
    assert_eq!(fetched_members.len(), 2);
    assert_eq!(fetched_members[0].ordinal, 0);
    assert_eq!(fetched_members[1].ordinal, 1);

    // Second version
    let (ver2, _) = db
        .create_collection_version_with_members(coll.id, "version_hash_2", user.id, &[])
        .await?;
    assert_eq!(ver2.version_number, 2);

    // Latest version
    let latest = db.get_latest_collection_version(coll.id).await?.unwrap();
    assert_eq!(latest.version_number, 2);

    // List versions
    let versions = db.list_collection_versions(coll.id).await?;
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0].version_number, 2); // DESC order

    // Yank
    let yanked = db.yank_collection(project.id, &name, "Bad data").await?;
    assert!(yanked);
    let found = db.get_collection(project.id, &name).await?.unwrap();
    assert!(found.yanked);
    assert_eq!(found.yank_reason, Some("Bad data".to_string()));

    Ok(())
}

// ========================================================================
// Endpoint Yank Operations
// ========================================================================

#[tokio::test]
async fn test_endpoint_yanks() -> Result<()> {
    let Some(db) = get_test_db().await else {
        return Ok(());
    };

    let user = db
        .upsert_user_from_github(
            (rand::random::<i64>() & i64::MAX),
            &unique_name("user"),
            None,
            None,
        )
        .await?;
    let project = db
        .create_project(user.id, &unique_name("proj"), None, "private")
        .await?;
    let sha = format!("{:040x}", rand::random::<u128>());
    let commit = db
        .insert_commit(
            project.id,
            "github",
            "user/repo",
            &sha,
            "hash",
            user.id,
            None,
        )
        .await?;

    // Not yanked initially
    assert!(
        !db.is_endpoint_yanked(project.id, "analysis", commit.id)
            .await?
    );

    // Yank
    let yank = db
        .insert_endpoint_yank(project.id, "analysis", commit.id, "Broken output", user.id)
        .await?;
    assert_eq!(yank.endpoint_name, "analysis");

    // Now yanked
    assert!(
        db.is_endpoint_yanked(project.id, "analysis", commit.id)
            .await?
    );

    // List
    let yanks = db.list_endpoint_yanks(project.id).await?;
    assert_eq!(yanks.len(), 1);

    // Remove yank
    let removed = db
        .remove_endpoint_yank(project.id, "analysis", commit.id)
        .await?;
    assert!(removed);
    assert!(
        !db.is_endpoint_yanked(project.id, "analysis", commit.id)
            .await?
    );

    Ok(())
}

// ========================================================================
// Secret Operations
// ========================================================================

#[tokio::test]
async fn test_secrets() -> Result<()> {
    let Some(db) = get_test_db().await else {
        return Ok(());
    };

    let user = db
        .upsert_user_from_github(
            (rand::random::<i64>() & i64::MAX),
            &unique_name("user"),
            None,
            None,
        )
        .await?;
    let project = db
        .create_project(user.id, &unique_name("proj"), None, "private")
        .await?;

    // Set a secret
    let secret = db
        .upsert_secret(project.id, "API_KEY", b"encrypted_value_1", user.id)
        .await?;
    assert_eq!(secret.name, "API_KEY");
    let version1 = secret.version_id;

    // List secrets (returns info without values)
    let infos = db.list_secrets(project.id).await?;
    assert_eq!(infos.len(), 1);
    assert_eq!(infos[0].name, "API_KEY");

    // Get secret by name (with encrypted value)
    let found = db.get_secret(project.id, "API_KEY").await?.unwrap();
    assert_eq!(found.encrypted_value, b"encrypted_value_1");

    // Upsert same name → new version_id
    let updated = db
        .upsert_secret(project.id, "API_KEY", b"encrypted_value_2", user.id)
        .await?;
    assert_ne!(updated.version_id, version1);
    assert_eq!(updated.encrypted_value, b"encrypted_value_2");

    // Delete
    let deleted = db.delete_secret(project.id, "API_KEY").await?;
    assert!(deleted);
    assert!(db.get_secret(project.id, "API_KEY").await?.is_none());
    assert!(db.list_secrets(project.id).await?.is_empty());

    Ok(())
}

// ========================================================================
// Environment Image Operations
// ========================================================================

#[tokio::test]
async fn test_environment_images() -> Result<()> {
    let Some(db) = get_test_db().await else {
        return Ok(());
    };

    let env_hash = format!("env_{:032x}", rand::random::<u128>());
    let img = db
        .insert_environment_image(
            &env_hash,
            "ghcr.io/ozzydb/envs/abc123",
            "base_lockfile",
            Some("python:3.12"),
        )
        .await?;
    assert_eq!(img.env_hash, env_hash);
    assert_eq!(img.build_type, "base_lockfile");
    assert!(img.built_at.is_none());

    // Get by hash
    let found = db.get_environment_image(&env_hash).await?.unwrap();
    assert_eq!(found.image_ref, "ghcr.io/ozzydb/envs/abc123");

    // Mark as built
    let marked = db
        .mark_environment_built(&env_hash, Some("logs/build.log"), 45000)
        .await?;
    assert!(marked);
    let found = db.get_environment_image(&env_hash).await?.unwrap();
    assert!(found.built_at.is_some());
    assert_eq!(found.build_duration_ms, Some(45000));

    Ok(())
}

// ========================================================================
// Source Cache Operations
// ========================================================================

#[tokio::test]
async fn test_source_cache() -> Result<()> {
    let Some(db) = get_test_db().await else {
        return Ok(());
    };

    let sha = format!("{:040x}", rand::random::<u128>());
    let entry = db
        .insert_source_cache(
            "github",
            "user/repo",
            &sha,
            &format!("source/{}", sha),
            5000,
        )
        .await?;
    assert_eq!(entry.git_commit_sha, sha);
    assert_eq!(entry.byte_size, 5000);

    // Get
    let found = db
        .get_source_cache("github", "user/repo", &sha)
        .await?
        .unwrap();
    assert_eq!(found.r2_key, format!("source/{}", sha));

    // Touch updates last_accessed
    let touched = db.touch_source_cache("github", "user/repo", &sha).await?;
    assert!(touched);

    // Upsert same key → updates last_accessed (no duplicate)
    let entry2 = db
        .insert_source_cache(
            "github",
            "user/repo",
            &sha,
            &format!("source/{}", sha),
            5000,
        )
        .await?;
    assert_eq!(entry2.id, entry.id);

    Ok(())
}

// ========================================================================
// Materialized Cache Operations
// ========================================================================

#[tokio::test]
async fn test_materialized_cache() -> Result<()> {
    let Some(db) = get_test_db().await else {
        return Ok(());
    };

    let user = db
        .upsert_user_from_github(
            (rand::random::<i64>() & i64::MAX),
            &unique_name("user"),
            None,
            None,
        )
        .await?;
    let project = db
        .create_project(user.id, &unique_name("proj"), None, "private")
        .await?;
    let sha = format!("{:040x}", rand::random::<u128>());
    let commit = db
        .insert_commit(
            project.id,
            "github",
            "user/repo",
            &sha,
            "hash",
            user.id,
            None,
        )
        .await?;

    let mat_hash = format!("mat_{:032x}", rand::random::<u128>());
    let entry = db
        .insert_materialized_cache(
            &mat_hash,
            project.id,
            commit.id,
            "analysis",
            "qc_node",
            "quality_control",
            "output_hash_123",
            "cache/output_hash_123",
            "application/vnd.apache.parquet",
            1024,
            "linux-x86_64",
            1,
        )
        .await?;
    assert_eq!(entry.materialized_hash, mat_hash);
    assert_eq!(entry.access_count, 1);

    // Get
    let found = db.get_materialized_cache(&mat_hash).await?.unwrap();
    assert_eq!(found.output_hash, "output_hash_123");

    // Touch increments access_count
    let touched = db.touch_materialized_cache(&mat_hash).await?;
    assert!(touched);
    let found = db.get_materialized_cache(&mat_hash).await?.unwrap();
    assert_eq!(found.access_count, 2);

    // List by project
    let entries = db.list_materialized_cache(project.id, None).await?;
    assert_eq!(entries.len(), 1);

    // List by project + endpoint
    let entries = db
        .list_materialized_cache(project.id, Some("analysis"))
        .await?;
    assert_eq!(entries.len(), 1);
    let entries = db
        .list_materialized_cache(project.id, Some("other"))
        .await?;
    assert!(entries.is_empty());

    // Upsert same hash → increments access_count
    let entry2 = db
        .insert_materialized_cache(
            &mat_hash,
            project.id,
            commit.id,
            "analysis",
            "qc_node",
            "quality_control",
            "output_hash_123",
            "cache/output_hash_123",
            "application/vnd.apache.parquet",
            1024,
            "linux-x86_64",
            1,
        )
        .await?;
    assert_eq!(entry2.access_count, 3); // was 2, upsert adds 1

    // Delete
    let deleted = db.delete_materialized_cache(&mat_hash).await?;
    assert!(deleted);
    assert!(db.get_materialized_cache(&mat_hash).await?.is_none());

    Ok(())
}
