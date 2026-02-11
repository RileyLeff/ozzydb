//! Database query implementations.

use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

use super::models::*;

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
        let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE github_id = $1")
            .bind(github_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(user)
    }

    /// Find user by username.
    pub async fn get_user_by_username(&self, username: &str) -> Result<Option<User>> {
        let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE username = $1")
            .bind(username)
            .fetch_optional(&self.pool)
            .await?;
        Ok(user)
    }

    /// Find user by ID.
    pub async fn get_user_by_id(&self, id: Uuid) -> Result<Option<User>> {
        let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(user)
    }

    /// Create or update user from GitHub OAuth.
    ///
    /// DESIGN NOTE: `username` is set to `github_login` on every login. If a user
    /// renames their GitHub account, their ozzy username silently changes too, which
    /// breaks existing `owner/project` references pointing at the old name. Before
    /// public launch, consider either (a) pinning username on first login only, or
    /// (b) adding a username-redirect / alias table so old references still resolve.
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
                username = EXCLUDED.username,
                github_login = EXCLUDED.github_login,
                email = COALESCE(EXCLUDED.email, users.email),
                avatar_url = EXCLUDED.avatar_url,
                updated_at = NOW()
            RETURNING *
            "#,
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
            "#,
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
            "#,
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
            "#,
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
    pub async fn list_user_projects_paginated(
        &self,
        user_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Project>> {
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

    /// Add or update a collaborator on a project.
    pub async fn upsert_project_collaborator(
        &self,
        project_id: Uuid,
        user_id: Uuid,
        permission: &str,
    ) -> Result<ProjectCollaborator> {
        let collaborator = sqlx::query_as::<_, ProjectCollaborator>(
            r#"
            INSERT INTO project_collaborators (project_id, user_id, permission)
            VALUES ($1, $2, $3)
            ON CONFLICT (project_id, user_id) DO UPDATE SET
                permission = EXCLUDED.permission
            RETURNING project_id, user_id, permission, created_at
            "#,
        )
        .bind(project_id)
        .bind(user_id)
        .bind(permission)
        .fetch_one(&self.pool)
        .await?;
        Ok(collaborator)
    }

    /// Remove a collaborator from a project.
    pub async fn remove_project_collaborator(
        &self,
        project_id: Uuid,
        user_id: Uuid,
    ) -> Result<bool> {
        let result =
            sqlx::query("DELETE FROM project_collaborators WHERE project_id = $1 AND user_id = $2")
                .bind(project_id)
                .bind(user_id)
                .execute(&self.pool)
                .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Get a collaborator permission entry.
    pub async fn get_project_collaborator(
        &self,
        project_id: Uuid,
        user_id: Uuid,
    ) -> Result<Option<ProjectCollaborator>> {
        let collaborator = sqlx::query_as::<_, ProjectCollaborator>(
            "SELECT project_id, user_id, permission, created_at FROM project_collaborators WHERE project_id = $1 AND user_id = $2"
        )
        .bind(project_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(collaborator)
    }

    /// List collaborators for a project.
    pub async fn list_project_collaborators(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<ProjectCollaboratorWithUser>> {
        let collaborators = sqlx::query_as::<_, ProjectCollaboratorWithUser>(
            r#"
            SELECT c.user_id, u.username, c.permission, c.created_at
            FROM project_collaborators c
            JOIN users u ON u.id = c.user_id
            WHERE c.project_id = $1
            ORDER BY u.username
            "#,
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(collaborators)
    }

    // ========================================================================
    // API Token Operations
    // ========================================================================

    /// Get token by hash.
    pub async fn get_token_by_hash(&self, token_hash: &str) -> Result<Option<ApiToken>> {
        let token = sqlx::query_as::<_, ApiToken>("SELECT * FROM api_tokens WHERE token_hash = $1")
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
            "#,
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
            "SELECT * FROM api_tokens WHERE user_id = $1 ORDER BY created_at DESC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(tokens)
    }

    /// Delete a token by ID.
    pub async fn delete_token(&self, user_id: Uuid, token_id: Uuid) -> Result<bool> {
        let result = sqlx::query("DELETE FROM api_tokens WHERE id = $1 AND user_id = $2")
            .bind(token_id)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Delete a token by name.
    pub async fn delete_token_by_name(&self, user_id: Uuid, name: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM api_tokens WHERE user_id = $1 AND name = $2")
            .bind(user_id)
            .bind(name)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Update last_used_at for a token.
    pub async fn touch_token(&self, token_id: Uuid) -> Result<()> {
        sqlx::query("UPDATE api_tokens SET last_used_at = NOW() WHERE id = $1")
            .bind(token_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
