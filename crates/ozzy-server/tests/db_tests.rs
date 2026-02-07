//! Database integration tests.
//!
//! These tests require a PostgreSQL database to run.
//! Set DATABASE_URL environment variable to run these tests.
//!
//! Example using Docker:
//! ```bash
//! docker run -p 5432:5432 -e POSTGRES_PASSWORD=test -e POSTGRES_DB=ozzy_test postgres:16-alpine
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

fn should_skip_db_tests() -> bool {
    env::var("DATABASE_URL").is_err()
}

#[tokio::test]
async fn test_user_crud() -> Result<()> {
    if should_skip_db_tests() {
        eprintln!("Skipping DB tests - DATABASE_URL not set");
        return Ok(());
    }

    let db = get_test_db().await.unwrap();

    // Create user via GitHub
    let github_id = rand::random::<i64>().abs();
    let username = format!("testuser_{}", github_id);

    let user = db
        .upsert_user_from_github(github_id, &username, Some("test@example.com"), None)
        .await?;

    assert_eq!(user.username, username);
    assert_eq!(user.github_id, Some(github_id));

    // Retrieve by GitHub ID
    let found = db.get_user_by_github_id(github_id).await?;
    assert!(found.is_some());
    assert_eq!(found.unwrap().id, user.id);

    // Retrieve by username
    let found = db.get_user_by_username(&username).await?;
    assert!(found.is_some());
    assert_eq!(found.unwrap().id, user.id);

    // Retrieve by ID
    let found = db.get_user_by_id(user.id).await?;
    assert!(found.is_some());

    // Update user
    let updated = db
        .upsert_user_from_github(
            github_id,
            &username,
            Some("new@example.com"),
            Some("https://avatar.url"),
        )
        .await?;
    assert_eq!(updated.id, user.id);
    assert_eq!(updated.avatar_url, Some("https://avatar.url".to_string()));

    Ok(())
}

#[tokio::test]
async fn test_project_crud() -> Result<()> {
    if should_skip_db_tests() {
        eprintln!("Skipping DB tests - DATABASE_URL not set");
        return Ok(());
    }

    let db = get_test_db().await.unwrap();

    // Create user first
    let github_id = rand::random::<i64>().abs();
    let username = format!("projowner_{}", github_id);
    let user = db
        .upsert_user_from_github(github_id, &username, None, None)
        .await?;

    // Create project
    let slug = format!("test-project-{}", Uuid::new_v4());
    let project = db
        .create_project(user.id, &slug, Some("A test project"), "private")
        .await?;

    assert_eq!(project.slug, slug);
    assert_eq!(project.owner_user_id, user.id);
    assert_eq!(project.visibility, "private");

    // Retrieve project
    let found = db.get_project(&username, &slug).await?;
    assert!(found.is_some());
    assert_eq!(found.unwrap().id, project.id);

    // List user projects
    let projects = db.list_user_projects(user.id).await?;
    assert!(projects.iter().any(|p| p.id == project.id));

    Ok(())
}

#[tokio::test]
async fn test_ref_operations() -> Result<()> {
    if should_skip_db_tests() {
        eprintln!("Skipping DB tests - DATABASE_URL not set");
        return Ok(());
    }

    let db = get_test_db().await.unwrap();

    // Create user and project
    let github_id = rand::random::<i64>().abs();
    let username = format!("refowner_{}", github_id);
    let user = db
        .upsert_user_from_github(github_id, &username, None, None)
        .await?;

    let slug = format!("ref-test-{}", Uuid::new_v4());
    let project = db.create_project(user.id, &slug, None, "public").await?;

    // Create a minimal commit
    let commit = ozzy_core::project::Commit {
        hash: format!("{:064x}", rand::random::<u64>()),
        parent_hashes: vec![],
        author: "test".to_string(),
        message: "Test commit".to_string(),
        timestamp: chrono::Utc::now(),
        data_sources: Default::default(),
        transforms: Default::default(),
        endpoints: Default::default(),
    };

    let commit_id = db
        .create_commit(
            project.id,
            &commit,
            Some(user.id),
            &std::collections::HashMap::new(),
        )
        .await?;

    // Create branch ref
    let branch = db
        .upsert_ref(project.id, "main", "branch", commit_id)
        .await?;
    assert_eq!(branch.name, "main");
    assert_eq!(branch.ref_type, "branch");

    // Create tag ref
    let tag = db
        .upsert_ref(project.id, "v1.0.0", "tag", commit_id)
        .await?;
    assert_eq!(tag.name, "v1.0.0");
    assert_eq!(tag.ref_type, "tag");

    // Get ref by name (should find branch first)
    let found = db.get_ref_by_name(project.id, "main").await?;
    assert!(found.is_some());
    assert_eq!(found.unwrap().ref_type, "branch");

    // List all refs
    let refs = db.list_refs(project.id).await?;
    assert!(refs.len() >= 2);

    Ok(())
}

#[tokio::test]
async fn test_api_token_operations() -> Result<()> {
    if should_skip_db_tests() {
        eprintln!("Skipping DB tests - DATABASE_URL not set");
        return Ok(());
    }

    let db = get_test_db().await.unwrap();

    // Create user
    let github_id = rand::random::<i64>().abs();
    let username = format!("tokenuser_{}", github_id);
    let user = db
        .upsert_user_from_github(github_id, &username, None, None)
        .await?;

    // Create token
    let token_hash = format!("{:064x}", rand::random::<u64>());
    let token = db
        .create_token(
            user.id,
            "test-token",
            &token_hash,
            "ozzy_abc",
            &["read".to_string(), "write".to_string()],
            None,
        )
        .await?;

    assert_eq!(token.name, "test-token");
    assert_eq!(token.scopes, vec!["read", "write"]);

    // Retrieve by hash
    let found = db.get_token_by_hash(&token_hash).await?;
    assert!(found.is_some());
    assert_eq!(found.unwrap().id, token.id);

    // List user tokens
    let tokens = db.list_user_tokens(user.id).await?;
    assert!(tokens.iter().any(|t| t.id == token.id));

    // Touch token
    db.touch_token(token.id).await?;

    // Delete by name
    let deleted = db.delete_token_by_name(user.id, "test-token").await?;
    assert!(deleted);

    // Verify deletion
    let found = db.get_token_by_hash(&token_hash).await?;
    assert!(found.is_none());

    Ok(())
}

#[tokio::test]
async fn test_commit_with_data() -> Result<()> {
    if should_skip_db_tests() {
        eprintln!("Skipping DB tests - DATABASE_URL not set");
        return Ok(());
    }

    let db = get_test_db().await.unwrap();

    // Create user and project
    let github_id = rand::random::<i64>().abs();
    let username = format!("commitowner_{}", github_id);
    let user = db
        .upsert_user_from_github(github_id, &username, None, None)
        .await?;

    let slug = format!("commit-test-{}", Uuid::new_v4());
    let project = db.create_project(user.id, &slug, None, "public").await?;

    // Create commit with data source and transform
    let mut data_sources = std::collections::BTreeMap::new();
    data_sources.insert(
        "raw".to_string(),
        ozzy_core::project::DataSource {
            name: "raw".to_string(),
            path: "data/raw.parquet".to_string(),
            hash: "abc123".to_string(),
            schema_hash: "schema123".to_string(),
            row_count: Some(1000),
            byte_size: Some(50000),
        },
    );

    let mut transforms = std::collections::BTreeMap::new();
    transforms.insert(
        "clean".to_string(),
        ozzy_core::project::Transform {
            name: "clean".to_string(),
            source_path: "transforms/clean.py".to_string(),
            hash: "def456".to_string(),
            source_hash: "src456".to_string(),
            function_name: "clean_data".to_string(),
            input_schema: None,
            output_schema: None,
            runtime: "python".to_string(),
            lockfile_hash: "lock123".to_string(),
            params_schema: Default::default(),
            reproducible: true,
        },
    );

    let mut endpoints = std::collections::BTreeMap::new();
    endpoints.insert(
        "clean_data".to_string(),
        ozzy_core::project::Endpoint {
            name: "clean_data".to_string(),
            description: Some("Clean raw data".to_string()),
            nodes: vec![],
            edges: vec![],
        },
    );

    let commit = ozzy_core::project::Commit {
        hash: format!("{:064x}", rand::random::<u64>()),
        parent_hashes: vec![],
        author: "test@example.com".to_string(),
        message: "Add data processing pipeline".to_string(),
        timestamp: chrono::Utc::now(),
        data_sources,
        transforms,
        endpoints,
    };

    let commit_id = db
        .create_commit(
            project.id,
            &commit,
            Some(user.id),
            &std::collections::HashMap::new(),
        )
        .await?;

    // Verify data sources
    let sources = db.get_data_sources(commit_id).await?;
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].name, "raw");
    assert_eq!(sources[0].content_hash, "abc123");

    // Verify transforms
    let transforms = db.get_transforms(commit_id).await?;
    assert_eq!(transforms.len(), 1);
    assert_eq!(transforms[0].name, "clean");
    assert_eq!(transforms[0].content_hash, "def456");

    // Verify endpoints
    let endpoints = db.get_endpoints(commit_id).await?;
    assert_eq!(endpoints.len(), 1);
    assert_eq!(endpoints[0].name, "clean_data");

    // Retrieve commit by hash
    let found = db.get_commit_by_hash(project.id, &commit.hash).await?;
    assert!(found.is_some());
    assert_eq!(found.unwrap().message, "Add data processing pipeline");

    Ok(())
}

#[tokio::test]
async fn test_list_commits_paginated() -> Result<()> {
    if should_skip_db_tests() {
        eprintln!("Skipping DB tests - DATABASE_URL not set");
        return Ok(());
    }

    let db = get_test_db().await.unwrap();

    let github_id = rand::random::<i64>().abs();
    let username = format!("historyowner_{}", github_id);
    let user = db
        .upsert_user_from_github(github_id, &username, None, None)
        .await?;

    let slug = format!("history-test-{}", Uuid::new_v4());
    let project = db.create_project(user.id, &slug, None, "private").await?;

    let commit1 = ozzy_core::project::Commit {
        hash: format!("{:064x}", rand::random::<u64>()),
        parent_hashes: vec![],
        author: "test".to_string(),
        message: "First commit".to_string(),
        timestamp: chrono::Utc::now(),
        data_sources: Default::default(),
        transforms: Default::default(),
        endpoints: Default::default(),
    };
    db.create_commit(
        project.id,
        &commit1,
        Some(user.id),
        &std::collections::HashMap::new(),
    )
    .await?;

    let commit2 = ozzy_core::project::Commit {
        hash: format!("{:064x}", rand::random::<u64>()),
        parent_hashes: vec![commit1.hash.clone()],
        author: "test".to_string(),
        message: "Second commit".to_string(),
        timestamp: chrono::Utc::now(),
        data_sources: Default::default(),
        transforms: Default::default(),
        endpoints: Default::default(),
    };
    db.create_commit(
        project.id,
        &commit2,
        Some(user.id),
        &std::collections::HashMap::new(),
    )
    .await?;

    let commits = db.list_commits_paginated(project.id, 10, 0).await?;
    assert!(!commits.is_empty());
    assert!(commits.iter().any(|c| c.hash == commit1.hash));
    assert!(commits.iter().any(|c| c.hash == commit2.hash));

    Ok(())
}

#[tokio::test]
async fn test_content_deduplication() -> Result<()> {
    if should_skip_db_tests() {
        eprintln!("Skipping DB tests - DATABASE_URL not set");
        return Ok(());
    }

    let db = get_test_db().await.unwrap();

    let content_hash = format!("{:064x}", rand::random::<u64>());

    // Initially should not exist
    let exists = db.content_exists(&content_hash).await?;
    assert!(!exists);

    // Register content
    db.register_content(&content_hash, "content/ab/cd/abc.parquet", "parquet", 10000)
        .await?;

    // Now should exist
    let exists = db.content_exists(&content_hash).await?;
    assert!(exists);

    // Register again (should increment ref_count)
    db.register_content(&content_hash, "content/ab/cd/abc.parquet", "parquet", 10000)
        .await?;

    Ok(())
}

#[tokio::test]
async fn test_project_collaborator_operations() -> Result<()> {
    if should_skip_db_tests() {
        eprintln!("Skipping DB tests - DATABASE_URL not set");
        return Ok(());
    }

    let db = get_test_db().await.unwrap();

    let owner_github_id = rand::random::<i64>().abs();
    let owner_name = format!("collab_owner_{}", owner_github_id);
    let owner = db
        .upsert_user_from_github(owner_github_id, &owner_name, None, None)
        .await?;

    let collab_github_id = rand::random::<i64>().abs();
    let collab_name = format!("collab_user_{}", collab_github_id);
    let collaborator = db
        .upsert_user_from_github(collab_github_id, &collab_name, None, None)
        .await?;

    let slug = format!("collab-test-{}", Uuid::new_v4());
    let project = db.create_project(owner.id, &slug, None, "private").await?;

    db.upsert_project_collaborator(project.id, collaborator.id, "write")
        .await?;
    let fetched = db
        .get_project_collaborator(project.id, collaborator.id)
        .await?
        .expect("collaborator should exist");
    assert_eq!(fetched.permission, "write");

    let all = db.list_project_collaborators(project.id).await?;
    assert!(all.iter().any(|c| c.user_id == collaborator.id));

    let removed = db
        .remove_project_collaborator(project.id, collaborator.id)
        .await?;
    assert!(removed);
    let missing = db
        .get_project_collaborator(project.id, collaborator.id)
        .await?;
    assert!(missing.is_none());

    Ok(())
}

#[tokio::test]
async fn test_lookup_data_schema_by_hash() -> Result<()> {
    if should_skip_db_tests() {
        eprintln!("Skipping DB tests - DATABASE_URL not set");
        return Ok(());
    }

    let db = get_test_db().await.unwrap();

    let github_id = rand::random::<i64>().abs();
    let username = format!("schema_lookup_{}", github_id);
    let user = db
        .upsert_user_from_github(github_id, &username, None, None)
        .await?;
    let slug = format!("schema-lookup-{}", Uuid::new_v4());
    let project = db.create_project(user.id, &slug, None, "private").await?;

    let mut data_sources = std::collections::BTreeMap::new();
    let content_hash = format!("{:064x}", rand::random::<u64>());
    data_sources.insert(
        "raw".to_string(),
        ozzy_core::project::DataSource {
            name: "raw".to_string(),
            path: "data/raw.parquet".to_string(),
            hash: content_hash.clone(),
            schema_hash: "schemahash".to_string(),
            row_count: Some(10),
            byte_size: Some(100),
        },
    );

    let commit = ozzy_core::project::Commit {
        hash: format!("{:064x}", rand::random::<u64>()),
        parent_hashes: vec![],
        author: "test".to_string(),
        message: "schema commit".to_string(),
        timestamp: chrono::Utc::now(),
        data_sources,
        transforms: Default::default(),
        endpoints: Default::default(),
    };

    let mut schemas = std::collections::HashMap::new();
    schemas.insert(
        content_hash.clone(),
        serde_json::json!({"fields": [{"name": "x", "type": "INT64"}]}),
    );
    db.create_commit(project.id, &commit, Some(user.id), &schemas)
        .await?;

    let found = db
        .get_data_schema_by_hash(&content_hash)
        .await?
        .expect("schema should exist");
    assert!(found.get("fields").is_some());

    Ok(())
}
