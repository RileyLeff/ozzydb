//! Database query implementations.

use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

use super::models::*;
use ozzy_core::project::Commit;

/// Database operations wrapper.
#[derive(Clone)]
pub struct Database {
    pool: PgPool,
}

impl Database {
    /// Create a new database wrapper.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Get the underlying pool.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    // ========================================================================
    // User Operations
    // ========================================================================

    /// Find user by GitHub ID.
    pub async fn get_user_by_github_id(&self, github_id: i64) -> Result<Option<User>> {
        let user = sqlx::query_as::<_, User>(
            "SELECT * FROM users WHERE github_id = $1"
        )
        .bind(github_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(user)
    }

    /// Find user by username.
    pub async fn get_user_by_username(&self, username: &str) -> Result<Option<User>> {
        let user = sqlx::query_as::<_, User>(
            "SELECT * FROM users WHERE username = $1"
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await?;
        Ok(user)
    }

    /// Find user by ID.
    pub async fn get_user_by_id(&self, id: Uuid) -> Result<Option<User>> {
        let user = sqlx::query_as::<_, User>(
            "SELECT * FROM users WHERE id = $1"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(user)
    }

    /// Create or update user from GitHub OAuth.
    pub async fn upsert_user_from_github(
        &self,
        github_id: i64,
        github_login: &str,
        email: Option<&str>,
        avatar_url: Option<&str>,
    ) -> Result<User> {
        let user = sqlx::query_as::<_, User>(
            r#"
            INSERT INTO users (id, username, email, github_id, github_login, avatar_url)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (github_id) DO UPDATE SET
                github_login = EXCLUDED.github_login,
                email = COALESCE(EXCLUDED.email, users.email),
                avatar_url = EXCLUDED.avatar_url,
                updated_at = NOW()
            RETURNING *
            "#
        )
        .bind(Uuid::new_v4())
        .bind(github_login)
        .bind(email)
        .bind(github_id)
        .bind(github_login)
        .bind(avatar_url)
        .fetch_one(&self.pool)
        .await?;
        Ok(user)
    }

    // ========================================================================
    // Project Operations
    // ========================================================================

    /// Get project by owner and slug.
    pub async fn get_project(&self, owner: &str, slug: &str) -> Result<Option<Project>> {
        let project = sqlx::query_as::<_, Project>(
            r#"
            SELECT p.* FROM projects p
            JOIN users u ON p.owner_user_id = u.id
            WHERE u.username = $1 AND p.slug = $2
            "#
        )
        .bind(owner)
        .bind(slug)
        .fetch_optional(&self.pool)
        .await?;
        Ok(project)
    }

    /// Create a new project.
    pub async fn create_project(
        &self,
        owner_id: Uuid,
        slug: &str,
        description: Option<&str>,
        visibility: &str,
    ) -> Result<Project> {
        let project = sqlx::query_as::<_, Project>(
            r#"
            INSERT INTO projects (id, owner_user_id, slug, description, visibility)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING *
            "#
        )
        .bind(Uuid::new_v4())
        .bind(owner_id)
        .bind(slug)
        .bind(description)
        .bind(visibility)
        .fetch_one(&self.pool)
        .await?;
        Ok(project)
    }

    /// Get or create a project atomically (upsert).
    pub async fn get_or_create_project(
        &self,
        owner_id: Uuid,
        slug: &str,
        visibility: &str,
    ) -> Result<Project> {
        let project = sqlx::query_as::<_, Project>(
            r#"
            INSERT INTO projects (id, owner_user_id, slug, visibility)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (owner_user_id, slug) DO UPDATE SET
                updated_at = NOW()
            RETURNING *
            "#
        )
        .bind(Uuid::new_v4())
        .bind(owner_id)
        .bind(slug)
        .bind(visibility)
        .fetch_one(&self.pool)
        .await?;
        Ok(project)
    }

    /// List projects for a user with pagination.
    pub async fn list_user_projects(&self, user_id: Uuid) -> Result<Vec<Project>> {
        self.list_user_projects_paginated(user_id, 50, 0).await
    }

    /// List projects for a user with explicit pagination.
    pub async fn list_user_projects_paginated(&self, user_id: Uuid, limit: i64, offset: i64) -> Result<Vec<Project>> {
        let projects = sqlx::query_as::<_, Project>(
            "SELECT * FROM projects WHERE owner_user_id = $1 ORDER BY updated_at DESC LIMIT $2 OFFSET $3"
        )
        .bind(user_id)
        .bind(limit.min(100)) // Cap at 100
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        Ok(projects)
    }

    // ========================================================================
    // Commit Operations
    // ========================================================================

    /// Create a commit and all its related records (transaction).
    /// `data_schemas` maps content_hash -> extracted Arrow schema JSON.
    pub async fn create_commit(
        &self,
        project_id: Uuid,
        commit: &Commit,
        author_id: Option<Uuid>,
        data_schemas: &std::collections::HashMap<String, serde_json::Value>,
    ) -> Result<Uuid> {
        let mut tx = self.pool.begin().await?;

        // Insert commit
        let commit_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO commits (id, project_id, hash, parent_hashes, author_id, author_name, message)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#
        )
        .bind(commit_id)
        .bind(project_id)
        .bind(&commit.hash)
        .bind(&commit.parent_hashes)
        .bind(author_id)
        .bind(&commit.author)
        .bind(&commit.message)
        .execute(&mut *tx)
        .await?;

        // Insert data sources
        for (name, ds) in &commit.data_sources {
            sqlx::query(
                r#"
                INSERT INTO data_sources (id, commit_id, name, content_hash, schema_hash, storage_key, schema_json, row_count, byte_size)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                "#
            )
            .bind(Uuid::new_v4())
            .bind(commit_id)
            .bind(name)
            .bind(&ds.hash)
            .bind(&ds.schema_hash)
            .bind(format!("data/{}.parquet", ds.hash))
            .bind(data_schemas.get(&ds.hash).cloned().unwrap_or_else(|| serde_json::json!({})))
            .bind(ds.row_count.map(|n| n as i64))
            .bind(ds.byte_size.map(|n| n as i64))
            .execute(&mut *tx)
            .await?;
        }

        // Insert transforms
        for (name, t) in &commit.transforms {
            sqlx::query(
                r#"
                INSERT INTO transforms (id, commit_id, name, content_hash, runtime, source_storage_key, function_name, lockfile_hash, params_schema, input_schema, output_schema, reproducible)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
                "#
            )
            .bind(Uuid::new_v4())
            .bind(commit_id)
            .bind(name)
            .bind(&t.hash)
            .bind(&t.runtime)
            .bind(format!("transforms/{}.py", t.hash))
            .bind(&t.function_name)
            .bind(&t.lockfile_hash)
            .bind(&t.params_schema)
            .bind(&t.input_schema)
            .bind(&t.output_schema)
            .bind(t.reproducible)
            .execute(&mut *tx)
            .await?;
        }

        // Insert endpoints
        for (name, ep) in &commit.endpoints {
            sqlx::query(
                r#"
                INSERT INTO endpoints (id, commit_id, name, description, definition)
                VALUES ($1, $2, $3, $4, $5)
                "#
            )
            .bind(Uuid::new_v4())
            .bind(commit_id)
            .bind(name)
            .bind(&ep.description)
            .bind(serde_json::to_value(ep)?)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(commit_id)
    }

    /// Get commit by hash.
    pub async fn get_commit_by_hash(&self, project_id: Uuid, hash: &str) -> Result<Option<DbCommit>> {
        let commit = sqlx::query_as::<_, DbCommit>(
            "SELECT * FROM commits WHERE project_id = $1 AND hash = $2"
        )
        .bind(project_id)
        .bind(hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(commit)
    }

    /// Get commit by ID.
    pub async fn get_commit_by_id(&self, commit_id: Uuid) -> Result<Option<DbCommit>> {
        let commit = sqlx::query_as::<_, DbCommit>(
            "SELECT * FROM commits WHERE id = $1"
        )
        .bind(commit_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(commit)
    }

    /// Get specific endpoint for a commit.
    pub async fn get_endpoint(&self, commit_id: Uuid, name: &str) -> Result<Option<DbEndpoint>> {
        let endpoint = sqlx::query_as::<_, DbEndpoint>(
            "SELECT * FROM endpoints WHERE commit_id = $1 AND name = $2"
        )
        .bind(commit_id)
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;
        Ok(endpoint)
    }

    /// Get data sources for a commit.
    pub async fn get_data_sources(&self, commit_id: Uuid) -> Result<Vec<DbDataSource>> {
        let sources = sqlx::query_as::<_, DbDataSource>(
            "SELECT * FROM data_sources WHERE commit_id = $1"
        )
        .bind(commit_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(sources)
    }

    /// Get transforms for a commit.
    pub async fn get_transforms(&self, commit_id: Uuid) -> Result<Vec<DbTransform>> {
        let transforms = sqlx::query_as::<_, DbTransform>(
            "SELECT * FROM transforms WHERE commit_id = $1"
        )
        .bind(commit_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(transforms)
    }

    /// Get endpoints for a commit.
    pub async fn get_endpoints(&self, commit_id: Uuid) -> Result<Vec<DbEndpoint>> {
        let endpoints = sqlx::query_as::<_, DbEndpoint>(
            "SELECT * FROM endpoints WHERE commit_id = $1"
        )
        .bind(commit_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(endpoints)
    }

    // ========================================================================
    // Ref Operations
    // ========================================================================

    /// Get ref by name and type.
    pub async fn get_ref(&self, project_id: Uuid, name: &str, ref_type: &str) -> Result<Option<DbRef>> {
        let r = sqlx::query_as::<_, DbRef>(
            "SELECT * FROM refs WHERE project_id = $1 AND name = $2 AND ref_type = $3"
        )
        .bind(project_id)
        .bind(name)
        .bind(ref_type)
        .fetch_optional(&self.pool)
        .await?;
        Ok(r)
    }

    /// Create or update a ref.
    pub async fn upsert_ref(
        &self,
        project_id: Uuid,
        name: &str,
        ref_type: &str,
        commit_id: Uuid,
    ) -> Result<DbRef> {
        let r = sqlx::query_as::<_, DbRef>(
            r#"
            INSERT INTO refs (id, project_id, name, ref_type, commit_id)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (project_id, name, ref_type) DO UPDATE SET
                commit_id = EXCLUDED.commit_id,
                updated_at = NOW()
            RETURNING *
            "#
        )
        .bind(Uuid::new_v4())
        .bind(project_id)
        .bind(name)
        .bind(ref_type)
        .bind(commit_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(r)
    }

    /// List all refs for a project.
    pub async fn list_refs(&self, project_id: Uuid) -> Result<Vec<DbRef>> {
        let refs = sqlx::query_as::<_, DbRef>(
            "SELECT * FROM refs WHERE project_id = $1 ORDER BY ref_type, name"
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(refs)
    }

    /// Get ref by name (tries branch first, then tag).
    /// Special handling: "@latest" resolves to the project's default_branch.
    pub async fn get_ref_by_name(&self, project_id: Uuid, name: &str) -> Result<Option<DbRef>> {
        // Handle @latest - resolve to the project's default_branch (usually "main")
        if name == "@latest" || name == "latest" {
            let default_branch: Option<String> = sqlx::query_scalar(
                "SELECT default_branch FROM projects WHERE id = $1"
            )
            .bind(project_id)
            .fetch_optional(&self.pool)
            .await?;

            let branch_name = default_branch.unwrap_or_else(|| "main".to_string());
            return self.get_ref(project_id, &branch_name, "branch").await;
        }

        // Try branch first
        if let Some(r) = self.get_ref(project_id, name, "branch").await? {
            return Ok(Some(r));
        }
        // Then try tag
        self.get_ref(project_id, name, "tag").await
    }

    // ========================================================================
    // API Token Operations
    // ========================================================================

    /// Get token by hash.
    pub async fn get_token_by_hash(&self, token_hash: &str) -> Result<Option<ApiToken>> {
        let token = sqlx::query_as::<_, ApiToken>(
            "SELECT * FROM api_tokens WHERE token_hash = $1"
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(token)
    }

    /// Create an API token.
    pub async fn create_token(
        &self,
        user_id: Uuid,
        name: &str,
        token_hash: &str,
        token_prefix: &str,
        scopes: &[String],
        expires_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<ApiToken> {
        let token = sqlx::query_as::<_, ApiToken>(
            r#"
            INSERT INTO api_tokens (id, user_id, name, token_hash, token_prefix, scopes, expires_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING *
            "#
        )
        .bind(Uuid::new_v4())
        .bind(user_id)
        .bind(name)
        .bind(token_hash)
        .bind(token_prefix)
        .bind(scopes)
        .bind(expires_at)
        .fetch_one(&self.pool)
        .await?;
        Ok(token)
    }

    /// List tokens for a user.
    pub async fn list_user_tokens(&self, user_id: Uuid) -> Result<Vec<ApiToken>> {
        let tokens = sqlx::query_as::<_, ApiToken>(
            "SELECT * FROM api_tokens WHERE user_id = $1 ORDER BY created_at DESC"
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(tokens)
    }

    /// Delete a token by ID.
    pub async fn delete_token(&self, user_id: Uuid, token_id: Uuid) -> Result<bool> {
        let result = sqlx::query(
            "DELETE FROM api_tokens WHERE id = $1 AND user_id = $2"
        )
        .bind(token_id)
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Delete a token by name.
    pub async fn delete_token_by_name(&self, user_id: Uuid, name: &str) -> Result<bool> {
        let result = sqlx::query(
            "DELETE FROM api_tokens WHERE user_id = $1 AND name = $2"
        )
        .bind(user_id)
        .bind(name)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Update last_used_at for a token.
    pub async fn touch_token(&self, token_id: Uuid) -> Result<()> {
        sqlx::query(
            "UPDATE api_tokens SET last_used_at = NOW() WHERE id = $1"
        )
        .bind(token_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ========================================================================
    // Content Deduplication
    // ========================================================================

    /// Check if content exists.
    pub async fn content_exists(&self, content_hash: &str) -> Result<bool> {
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM content_refs WHERE content_hash = $1)"
        )
        .bind(content_hash)
        .fetch_one(&self.pool)
        .await?;
        Ok(exists)
    }

    /// Get total byte size for a list of content hashes.
    pub async fn get_content_total_size(&self, hashes: &[String]) -> Result<i64> {
        if hashes.is_empty() {
            return Ok(0);
        }
        let total: Option<i64> = sqlx::query_scalar(
            "SELECT COALESCE(SUM(byte_size), 0) FROM content_refs WHERE content_hash = ANY($1)"
        )
        .bind(hashes)
        .fetch_one(&self.pool)
        .await?;
        Ok(total.unwrap_or(0))
    }

    /// Register content reference.
    pub async fn register_content(
        &self,
        content_hash: &str,
        storage_key: &str,
        content_type: &str,
        byte_size: i64,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO content_refs (content_hash, storage_key, content_type, byte_size)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (content_hash) DO UPDATE SET
                ref_count = content_refs.ref_count + 1
            "#
        )
        .bind(content_hash)
        .bind(storage_key)
        .bind(content_type)
        .bind(byte_size)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
