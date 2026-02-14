//! Database query implementations for v2 schema.

use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

use ozzy_core::hash::collection_hash;

use super::models::*;

/// Result of an atomic collection mutation.
/// Separates business-logic rejections (yanked, cycle) from internal errors
/// so the API handler can map to the correct HTTP status code.
pub enum CollectionMutResult<T> {
    Ok(T),
    Yanked(String),
    CycleDetected(String),
}

/// Database operations wrapper.
#[derive(Clone)]
pub struct Database {
    pool: PgPool,
}

impl Database {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    // ========================================================================
    // User Operations
    // ========================================================================

    pub async fn get_user_by_github_id(&self, github_id: i64) -> Result<Option<User>> {
        let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE github_id = $1")
            .bind(github_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(user)
    }

    pub async fn get_user_by_username(&self, username: &str) -> Result<Option<User>> {
        let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE username = $1")
            .bind(username)
            .fetch_optional(&self.pool)
            .await?;
        Ok(user)
    }

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
            INSERT INTO users (id, github_id, username, display_name, email, avatar_url)
            VALUES ($1, $2, $3, $3, $4, $5)
            ON CONFLICT (github_id) DO UPDATE SET
                username = EXCLUDED.username,
                display_name = EXCLUDED.display_name,
                email = COALESCE(EXCLUDED.email, users.email),
                avatar_url = EXCLUDED.avatar_url,
                updated_at = now()
            RETURNING *
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(github_id)
        .bind(github_login)
        .bind(email)
        .bind(avatar_url)
        .fetch_one(&self.pool)
        .await?;
        Ok(user)
    }

    // ========================================================================
    // Project Operations
    // ========================================================================

    pub async fn get_project(&self, owner: &str, slug: &str) -> Result<Option<Project>> {
        let project = sqlx::query_as::<_, Project>(
            r#"
            SELECT p.* FROM projects p
            JOIN users u ON p.owner_id = u.id
            WHERE u.username = $1 AND p.slug = $2
            "#,
        )
        .bind(owner)
        .bind(slug)
        .fetch_optional(&self.pool)
        .await?;
        Ok(project)
    }

    pub async fn get_project_by_id(&self, id: Uuid) -> Result<Option<Project>> {
        let project = sqlx::query_as::<_, Project>("SELECT * FROM projects WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(project)
    }

    pub async fn create_project(
        &self,
        owner_id: Uuid,
        slug: &str,
        description: Option<&str>,
        visibility: &str,
    ) -> Result<Project> {
        let project = sqlx::query_as::<_, Project>(
            r#"
            INSERT INTO projects (id, owner_id, slug, description, visibility)
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

    pub async fn get_or_create_project(
        &self,
        owner_id: Uuid,
        slug: &str,
        visibility: &str,
    ) -> Result<Project> {
        let project = sqlx::query_as::<_, Project>(
            r#"
            INSERT INTO projects (id, owner_id, slug, visibility)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (owner_id, slug) DO UPDATE SET
                updated_at = now()
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

    pub async fn list_user_projects(&self, user_id: Uuid) -> Result<Vec<Project>> {
        self.list_user_projects_paginated(user_id, 50, 0).await
    }

    pub async fn list_user_projects_paginated(
        &self,
        user_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Project>> {
        let projects = sqlx::query_as::<_, Project>(
            "SELECT * FROM projects WHERE owner_id = $1 ORDER BY updated_at DESC LIMIT $2 OFFSET $3",
        )
        .bind(user_id)
        .bind(limit.min(100))
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        Ok(projects)
    }

    // ========================================================================
    // Collaborator Operations
    // ========================================================================

    pub async fn upsert_project_collaborator(
        &self,
        project_id: Uuid,
        user_id: Uuid,
        role: &str,
    ) -> Result<ProjectCollaborator> {
        let collaborator = sqlx::query_as::<_, ProjectCollaborator>(
            r#"
            INSERT INTO project_collaborators (project_id, user_id, role)
            VALUES ($1, $2, $3)
            ON CONFLICT (project_id, user_id) DO UPDATE SET
                role = EXCLUDED.role
            RETURNING project_id, user_id, role, created_at
            "#,
        )
        .bind(project_id)
        .bind(user_id)
        .bind(role)
        .fetch_one(&self.pool)
        .await?;
        Ok(collaborator)
    }

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

    pub async fn get_project_collaborator(
        &self,
        project_id: Uuid,
        user_id: Uuid,
    ) -> Result<Option<ProjectCollaborator>> {
        let collaborator = sqlx::query_as::<_, ProjectCollaborator>(
            "SELECT project_id, user_id, role, created_at FROM project_collaborators WHERE project_id = $1 AND user_id = $2",
        )
        .bind(project_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(collaborator)
    }

    pub async fn list_project_collaborators(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<ProjectCollaboratorWithUser>> {
        let collaborators = sqlx::query_as::<_, ProjectCollaboratorWithUser>(
            r#"
            SELECT c.user_id, u.username, c.role, c.created_at
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

    pub async fn get_token_by_hash(&self, token_hash: &str) -> Result<Option<ApiToken>> {
        let token = sqlx::query_as::<_, ApiToken>("SELECT * FROM api_tokens WHERE token_hash = $1")
            .bind(token_hash)
            .fetch_optional(&self.pool)
            .await?;
        Ok(token)
    }

    pub async fn create_token(
        &self,
        user_id: Uuid,
        name: &str,
        token_hash: &str,
        scope: &str,
        project_id: Option<Uuid>,
        expires_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<ApiToken> {
        let token = sqlx::query_as::<_, ApiToken>(
            r#"
            INSERT INTO api_tokens (id, user_id, name, token_hash, scope, project_id, expires_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING *
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(user_id)
        .bind(name)
        .bind(token_hash)
        .bind(scope)
        .bind(project_id)
        .bind(expires_at)
        .fetch_one(&self.pool)
        .await?;
        Ok(token)
    }

    /// Upsert a session token (login flow). Avoids race conditions from concurrent logins.
    pub async fn upsert_session_token(
        &self,
        user_id: Uuid,
        name: &str,
        token_hash: &str,
        expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO api_tokens (id, user_id, name, token_hash, scope, project_id, expires_at)
            VALUES ($1, $2, $3, $4, 'account', NULL, $5)
            ON CONFLICT (user_id, name) DO UPDATE SET
                token_hash = EXCLUDED.token_hash,
                scope = EXCLUDED.scope,
                project_id = NULL,
                expires_at = EXCLUDED.expires_at
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(user_id)
        .bind(name)
        .bind(token_hash)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_user_tokens(&self, user_id: Uuid) -> Result<Vec<ApiToken>> {
        let tokens = sqlx::query_as::<_, ApiToken>(
            "SELECT * FROM api_tokens WHERE user_id = $1 ORDER BY created_at DESC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(tokens)
    }

    pub async fn delete_token(&self, user_id: Uuid, token_id: Uuid) -> Result<bool> {
        let result = sqlx::query("DELETE FROM api_tokens WHERE id = $1 AND user_id = $2")
            .bind(token_id)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn delete_token_by_name(&self, user_id: Uuid, name: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM api_tokens WHERE user_id = $1 AND name = $2")
            .bind(user_id)
            .bind(name)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn touch_token(&self, token_id: Uuid) -> Result<()> {
        sqlx::query("UPDATE api_tokens SET last_used_at = now() WHERE id = $1")
            .bind(token_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ========================================================================
    // Commit Operations
    // ========================================================================

    pub async fn insert_commit(
        &self,
        project_id: Uuid,
        git_provider: &str,
        git_repo: &str,
        git_commit_sha: &str,
        ozzy_toml_hash: &str,
        pushed_by: Uuid,
        message: Option<&str>,
    ) -> Result<Commit> {
        let commit = sqlx::query_as::<_, Commit>(
            r#"
            INSERT INTO commits (id, project_id, git_provider, git_repo, git_commit_sha, ozzy_toml_hash, pushed_by, message)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING *
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(project_id)
        .bind(git_provider)
        .bind(git_repo)
        .bind(git_commit_sha)
        .bind(ozzy_toml_hash)
        .bind(pushed_by)
        .bind(message)
        .fetch_one(&self.pool)
        .await?;
        Ok(commit)
    }

    pub async fn get_commit_by_id(&self, commit_id: Uuid) -> Result<Option<Commit>> {
        let commit = sqlx::query_as::<_, Commit>("SELECT * FROM commits WHERE id = $1")
            .bind(commit_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(commit)
    }

    pub async fn get_commit_by_sha(&self, project_id: Uuid, sha: &str) -> Result<Option<Commit>> {
        let commit = sqlx::query_as::<_, Commit>(
            "SELECT * FROM commits WHERE project_id = $1 AND git_commit_sha = $2",
        )
        .bind(project_id)
        .bind(sha)
        .fetch_optional(&self.pool)
        .await?;
        Ok(commit)
    }

    pub async fn count_commits(&self, project_id: Uuid) -> Result<i64> {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM commits WHERE project_id = $1")
            .bind(project_id)
            .fetch_one(&self.pool)
            .await?;
        Ok(row.0)
    }

    pub async fn list_commits(&self, project_id: Uuid, limit: i64) -> Result<Vec<Commit>> {
        let commits = sqlx::query_as::<_, Commit>(
            "SELECT * FROM commits WHERE project_id = $1 ORDER BY created_at DESC LIMIT $2",
        )
        .bind(project_id)
        .bind(limit.min(100))
        .fetch_all(&self.pool)
        .await?;
        Ok(commits)
    }

    // ========================================================================
    // Commit State Operations
    // ========================================================================

    pub async fn insert_commit_state(
        &self,
        commit_id: Uuid,
        ozzy_toml_raw: &str,
        environments: &serde_json::Value,
        transforms: &serde_json::Value,
        endpoints: &serde_json::Value,
        project_meta: &serde_json::Value,
    ) -> Result<CommitState> {
        let state = sqlx::query_as::<_, CommitState>(
            r#"
            INSERT INTO commit_state (commit_id, ozzy_toml_raw, environments, transforms, endpoints, project_meta)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING *
            "#,
        )
        .bind(commit_id)
        .bind(ozzy_toml_raw)
        .bind(environments)
        .bind(transforms)
        .bind(endpoints)
        .bind(project_meta)
        .fetch_one(&self.pool)
        .await?;
        Ok(state)
    }

    /// Register a commit atomically: insert commit + commit_state + optional ref update.
    /// If any step fails, the entire operation is rolled back.
    pub async fn register_commit_atomically(
        &self,
        project_id: Uuid,
        git_provider: &str,
        git_repo: &str,
        git_commit_sha: &str,
        ozzy_toml_hash: &str,
        pushed_by: Uuid,
        message: Option<&str>,
        ozzy_toml_raw: &str,
        environments: &serde_json::Value,
        transforms: &serde_json::Value,
        endpoints: &serde_json::Value,
        project_meta: &serde_json::Value,
        ref_name: Option<&str>,
    ) -> Result<Commit> {
        let mut tx = self.pool.begin().await?;

        let commit = sqlx::query_as::<_, Commit>(
            r#"
            INSERT INTO commits (id, project_id, git_provider, git_repo, git_commit_sha, ozzy_toml_hash, pushed_by, message)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING *
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(project_id)
        .bind(git_provider)
        .bind(git_repo)
        .bind(git_commit_sha)
        .bind(ozzy_toml_hash)
        .bind(pushed_by)
        .bind(message)
        .fetch_one(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO commit_state (commit_id, ozzy_toml_raw, environments, transforms, endpoints, project_meta)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(commit.id)
        .bind(ozzy_toml_raw)
        .bind(environments)
        .bind(transforms)
        .bind(endpoints)
        .bind(project_meta)
        .execute(&mut *tx)
        .await?;

        if let Some(ref_name) = ref_name {
            sqlx::query(
                r#"
                INSERT INTO refs (id, project_id, ref_name, ref_type, commit_id)
                VALUES ($1, $2, $3, $4, $5)
                ON CONFLICT (project_id, ref_name) DO UPDATE SET
                    commit_id = EXCLUDED.commit_id,
                    ref_type = EXCLUDED.ref_type,
                    updated_at = now()
                "#,
            )
            .bind(Uuid::new_v4())
            .bind(project_id)
            .bind(ref_name)
            .bind("branch")
            .bind(commit.id)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(commit)
    }

    pub async fn get_commit_state(&self, commit_id: Uuid) -> Result<Option<CommitState>> {
        let state =
            sqlx::query_as::<_, CommitState>("SELECT * FROM commit_state WHERE commit_id = $1")
                .bind(commit_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(state)
    }

    // ========================================================================
    // Ref Operations
    // ========================================================================

    pub async fn upsert_ref(
        &self,
        project_id: Uuid,
        ref_name: &str,
        ref_type: &str,
        commit_id: Uuid,
    ) -> Result<Ref> {
        let r = sqlx::query_as::<_, Ref>(
            r#"
            INSERT INTO refs (id, project_id, ref_name, ref_type, commit_id)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (project_id, ref_name) DO UPDATE SET
                commit_id = EXCLUDED.commit_id,
                ref_type = EXCLUDED.ref_type,
                updated_at = now()
            RETURNING *
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(project_id)
        .bind(ref_name)
        .bind(ref_type)
        .bind(commit_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(r)
    }

    pub async fn resolve_ref(&self, project_id: Uuid, ref_name: &str) -> Result<Option<Ref>> {
        let r =
            sqlx::query_as::<_, Ref>("SELECT * FROM refs WHERE project_id = $1 AND ref_name = $2")
                .bind(project_id)
                .bind(ref_name)
                .fetch_optional(&self.pool)
                .await?;
        Ok(r)
    }

    pub async fn list_refs(&self, project_id: Uuid) -> Result<Vec<Ref>> {
        let refs =
            sqlx::query_as::<_, Ref>("SELECT * FROM refs WHERE project_id = $1 ORDER BY ref_name")
                .bind(project_id)
                .fetch_all(&self.pool)
                .await?;
        Ok(refs)
    }

    pub async fn delete_ref(&self, project_id: Uuid, ref_name: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM refs WHERE project_id = $1 AND ref_name = $2")
            .bind(project_id)
            .bind(ref_name)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    // ========================================================================
    // Data Atom Operations
    // ========================================================================

    pub async fn insert_data_atom(
        &self,
        project_id: Uuid,
        name: &str,
        hash: &str,
        content_type: &str,
        byte_size: i64,
        r2_key: &str,
        uploaded_by: Uuid,
    ) -> Result<DataAtom> {
        let atom = sqlx::query_as::<_, DataAtom>(
            r#"
            INSERT INTO data_atoms (id, project_id, name, hash, content_type, byte_size, r2_key, uploaded_by)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING *
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(project_id)
        .bind(name)
        .bind(hash)
        .bind(content_type)
        .bind(byte_size)
        .bind(r2_key)
        .bind(uploaded_by)
        .fetch_one(&self.pool)
        .await?;
        Ok(atom)
    }

    pub async fn get_data_atom(&self, project_id: Uuid, name: &str) -> Result<Option<DataAtom>> {
        let atom = sqlx::query_as::<_, DataAtom>(
            "SELECT * FROM data_atoms WHERE project_id = $1 AND name = $2",
        )
        .bind(project_id)
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;
        Ok(atom)
    }

    pub async fn list_data_atoms(&self, project_id: Uuid) -> Result<Vec<DataAtom>> {
        let atoms = sqlx::query_as::<_, DataAtom>(
            "SELECT * FROM data_atoms WHERE project_id = $1 ORDER BY name",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(atoms)
    }

    pub async fn yank_data_atom(&self, project_id: Uuid, name: &str, reason: &str) -> Result<bool> {
        let mut tx = self.pool.begin().await?;

        // Advisory lock to serialize with collection add/remove operations
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1::text))")
            .bind(project_id)
            .execute(&mut *tx)
            .await?;

        let result = sqlx::query(
            "UPDATE data_atoms SET yanked = true, yank_reason = $3, yanked_at = now() WHERE project_id = $1 AND name = $2",
        )
        .bind(project_id)
        .bind(name)
        .bind(reason)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(result.rows_affected() > 0)
    }

    // ========================================================================
    // Content Ref Operations (deduplication)
    // ========================================================================

    pub async fn get_content_ref(&self, hash: &str) -> Result<Option<ContentRef>> {
        let cr = sqlx::query_as::<_, ContentRef>("SELECT * FROM content_refs WHERE hash = $1")
            .bind(hash)
            .fetch_optional(&self.pool)
            .await?;
        Ok(cr)
    }

    pub async fn upsert_content_ref(
        &self,
        hash: &str,
        r2_key: &str,
        content_type: &str,
        byte_size: i64,
    ) -> Result<ContentRef> {
        let cr = sqlx::query_as::<_, ContentRef>(
            r#"
            INSERT INTO content_refs (hash, r2_key, content_type, byte_size)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (hash) DO UPDATE SET
                ref_count = content_refs.ref_count + 1
            RETURNING *
            "#,
        )
        .bind(hash)
        .bind(r2_key)
        .bind(content_type)
        .bind(byte_size)
        .fetch_one(&self.pool)
        .await?;
        Ok(cr)
    }

    // ========================================================================
    // Data Metadata Operations
    // ========================================================================

    pub async fn append_metadata(
        &self,
        data_atom_id: Uuid,
        field: &str,
        value: &serde_json::Value,
        set_by: Uuid,
    ) -> Result<DataMetadataEntry> {
        let entry = sqlx::query_as::<_, DataMetadataEntry>(
            r#"
            INSERT INTO data_metadata_log (id, data_atom_id, field, value, set_by)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING *
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(data_atom_id)
        .bind(field)
        .bind(value)
        .bind(set_by)
        .fetch_one(&self.pool)
        .await?;
        Ok(entry)
    }

    pub async fn get_latest_metadata(
        &self,
        data_atom_id: Uuid,
        field: &str,
    ) -> Result<Option<DataMetadataEntry>> {
        let entry = sqlx::query_as::<_, DataMetadataEntry>(
            "SELECT * FROM data_metadata_log WHERE data_atom_id = $1 AND field = $2 ORDER BY created_at DESC LIMIT 1",
        )
        .bind(data_atom_id)
        .bind(field)
        .fetch_optional(&self.pool)
        .await?;
        Ok(entry)
    }

    pub async fn get_metadata_history(
        &self,
        data_atom_id: Uuid,
        field: &str,
    ) -> Result<Vec<DataMetadataEntry>> {
        let entries = sqlx::query_as::<_, DataMetadataEntry>(
            "SELECT * FROM data_metadata_log WHERE data_atom_id = $1 AND field = $2 ORDER BY created_at DESC",
        )
        .bind(data_atom_id)
        .bind(field)
        .fetch_all(&self.pool)
        .await?;
        Ok(entries)
    }

    /// Get the latest metadata entry for each field on a data atom.
    pub async fn get_all_latest_metadata(
        &self,
        data_atom_id: Uuid,
    ) -> Result<Vec<DataMetadataEntry>> {
        let entries = sqlx::query_as::<_, DataMetadataEntry>(
            r#"
            SELECT DISTINCT ON (field) *
            FROM data_metadata_log
            WHERE data_atom_id = $1
            ORDER BY field, created_at DESC
            "#,
        )
        .bind(data_atom_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(entries)
    }

    /// Get full metadata history across all fields for a data atom.
    pub async fn get_full_metadata_history(
        &self,
        data_atom_id: Uuid,
    ) -> Result<Vec<DataMetadataEntry>> {
        let entries = sqlx::query_as::<_, DataMetadataEntry>(
            "SELECT * FROM data_metadata_log WHERE data_atom_id = $1 ORDER BY created_at DESC",
        )
        .bind(data_atom_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(entries)
    }

    // ========================================================================
    // GitHub Installation Operations
    // ========================================================================

    pub async fn upsert_github_installation(
        &self,
        installation_id: i64,
        account_type: &str,
        account_login: &str,
    ) -> Result<GitHubInstallation> {
        let inst = sqlx::query_as::<_, GitHubInstallation>(
            r#"
            INSERT INTO github_installations (id, installation_id, account_type, account_login)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (installation_id) DO UPDATE SET
                account_type = EXCLUDED.account_type,
                account_login = EXCLUDED.account_login,
                updated_at = now()
            RETURNING *
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(installation_id)
        .bind(account_type)
        .bind(account_login)
        .fetch_one(&self.pool)
        .await?;
        Ok(inst)
    }

    pub async fn get_github_installation_by_login(
        &self,
        login: &str,
    ) -> Result<Option<GitHubInstallation>> {
        let inst = sqlx::query_as::<_, GitHubInstallation>(
            "SELECT * FROM github_installations WHERE account_login = $1",
        )
        .bind(login)
        .fetch_optional(&self.pool)
        .await?;
        Ok(inst)
    }

    pub async fn delete_github_installation(&self, installation_id: i64) -> Result<bool> {
        let result = sqlx::query("DELETE FROM github_installations WHERE installation_id = $1")
            .bind(installation_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    // ========================================================================
    // Project Update/Delete Operations
    // ========================================================================

    pub async fn update_project(
        &self,
        project_id: Uuid,
        description: Option<&str>,
        visibility: &str,
    ) -> Result<Option<Project>> {
        let project = sqlx::query_as::<_, Project>(
            r#"
            UPDATE projects SET description = $2, visibility = $3, updated_at = now()
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(project_id)
        .bind(description)
        .bind(visibility)
        .fetch_optional(&self.pool)
        .await?;
        Ok(project)
    }

    pub async fn delete_project(&self, project_id: Uuid) -> Result<bool> {
        let result = sqlx::query("DELETE FROM projects WHERE id = $1")
            .bind(project_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    // ========================================================================
    // Collection Operations
    // ========================================================================

    pub async fn create_collection(
        &self,
        project_id: Uuid,
        name: &str,
        created_by: Uuid,
    ) -> Result<Collection> {
        let coll = sqlx::query_as::<_, Collection>(
            r#"
            INSERT INTO collections (id, project_id, name, created_by)
            VALUES ($1, $2, $3, $4)
            RETURNING *
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(project_id)
        .bind(name)
        .bind(created_by)
        .fetch_one(&self.pool)
        .await?;
        Ok(coll)
    }

    pub async fn get_collection(&self, project_id: Uuid, name: &str) -> Result<Option<Collection>> {
        let coll = sqlx::query_as::<_, Collection>(
            "SELECT * FROM collections WHERE project_id = $1 AND name = $2",
        )
        .bind(project_id)
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;
        Ok(coll)
    }

    pub async fn list_collections(&self, project_id: Uuid) -> Result<Vec<Collection>> {
        let colls = sqlx::query_as::<_, Collection>(
            "SELECT * FROM collections WHERE project_id = $1 ORDER BY name",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(colls)
    }

    pub async fn yank_collection(
        &self,
        project_id: Uuid,
        name: &str,
        reason: &str,
    ) -> Result<bool> {
        let mut tx = self.pool.begin().await?;

        // Advisory lock to serialize with collection add/remove operations
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1::text))")
            .bind(project_id)
            .execute(&mut *tx)
            .await?;

        let result = sqlx::query(
            "UPDATE collections SET yanked = true, yank_reason = $3, yanked_at = now() WHERE project_id = $1 AND name = $2",
        )
        .bind(project_id)
        .bind(name)
        .bind(reason)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(result.rows_affected() > 0)
    }

    /// Create a new collection version with members, atomically in a transaction.
    /// Computes the next version number, inserts the version row, and all member rows.
    /// If any step fails, the entire operation is rolled back.
    pub async fn create_collection_version_with_members(
        &self,
        collection_id: Uuid,
        hash: &str,
        created_by: Uuid,
        members: &[(String, String, String, i32)], // (member_type, member_ref, member_hash, ordinal)
    ) -> Result<(CollectionVersion, Vec<CollectionMember>)> {
        let mut tx = self.pool.begin().await?;

        // Lock the collection row to prevent concurrent version number races
        sqlx::query("SELECT id FROM collections WHERE id = $1 FOR UPDATE")
            .bind(collection_id)
            .fetch_one(&mut *tx)
            .await?;

        let ver = sqlx::query_as::<_, CollectionVersion>(
            r#"
            INSERT INTO collection_versions (id, collection_id, version_number, hash, created_by)
            VALUES (
                $1, $2,
                COALESCE((SELECT MAX(version_number) FROM collection_versions WHERE collection_id = $2), 0) + 1,
                $3, $4
            )
            RETURNING *
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(collection_id)
        .bind(hash)
        .bind(created_by)
        .fetch_one(&mut *tx)
        .await?;

        let mut results = Vec::with_capacity(members.len());
        for (member_type, member_ref, member_hash, ordinal) in members {
            let member = sqlx::query_as::<_, CollectionMember>(
                r#"
                INSERT INTO collection_members (id, collection_version_id, member_type, member_ref, member_hash, ordinal)
                VALUES ($1, $2, $3, $4, $5, $6)
                RETURNING *
                "#,
            )
            .bind(Uuid::new_v4())
            .bind(ver.id)
            .bind(member_type)
            .bind(member_ref)
            .bind(member_hash)
            .bind(ordinal)
            .fetch_one(&mut *tx)
            .await?;
            results.push(member);
        }

        tx.commit().await?;
        Ok((ver, results))
    }

    /// Add members to a collection atomically.
    ///
    /// Acquires a project-level advisory lock to serialize all collection mutations
    /// within a project (prevents cycle TOCTOU), locks the collection row, re-checks
    /// yanked status, performs cycle detection (for collection-type members), reads
    /// current members, merges (deduplicating by type+ref), computes the collection
    /// hash, and creates a new version — all inside a single transaction.
    pub async fn add_to_collection_atomically(
        &self,
        project_id: Uuid,
        collection_id: Uuid,
        collection_name: &str,
        created_by: Uuid,
        new_members: &[(String, String, String)], // (member_type, member_ref, member_hash)
    ) -> Result<CollectionMutResult<(CollectionVersion, Vec<CollectionMember>)>> {
        let mut tx = self.pool.begin().await?;

        // Advisory lock serializes all collection mutations within a project,
        // preventing TOCTOU on cycle detection and yank checks.
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1::text))")
            .bind(project_id)
            .execute(&mut *tx)
            .await?;

        // Lock collection row and re-check yanked status
        let coll =
            sqlx::query_as::<_, Collection>("SELECT * FROM collections WHERE id = $1 FOR UPDATE")
                .bind(collection_id)
                .fetch_one(&mut *tx)
                .await?;

        if coll.yanked {
            return Ok(CollectionMutResult::Yanked(coll.name));
        }

        // Cycle detection for collection-type members, using the transaction
        // connection to avoid pool exhaustion deadlocks under contention.
        for (mtype, mref, _) in new_members {
            if mtype == "collection" {
                if Self::would_create_collection_cycle(&mut *tx, project_id, collection_name, mref)
                    .await?
                {
                    return Ok(CollectionMutResult::CycleDetected(mref.clone()));
                }
            }
        }

        // Read current members inside the lock
        let latest_ver = sqlx::query_as::<_, CollectionVersion>(
            "SELECT * FROM collection_versions WHERE collection_id = $1 ORDER BY version_number DESC LIMIT 1",
        )
        .bind(collection_id)
        .fetch_optional(&mut *tx)
        .await?;

        let current = if let Some(ref ver) = latest_ver {
            sqlx::query_as::<_, CollectionMember>(
                "SELECT * FROM collection_members WHERE collection_version_id = $1 ORDER BY ordinal",
            )
            .bind(ver.id)
            .fetch_all(&mut *tx)
            .await?
        } else {
            Vec::new()
        };

        // Merge: keep existing, append new (skip duplicates by type+ref AND by hash)
        let mut all: Vec<(String, String, String, i32)> = current
            .iter()
            .map(|m| {
                (
                    m.member_type.clone(),
                    m.member_ref.clone(),
                    m.member_hash.clone(),
                    m.ordinal,
                )
            })
            .collect();

        let mut next_ordinal = all.iter().map(|(_, _, _, o)| *o).max().unwrap_or(-1) + 1;
        for (mtype, mref, mhash) in new_members {
            // Skip if same type+ref already exists
            if all.iter().any(|(t, r, _, _)| t == mtype && r == mref) {
                continue;
            }
            // Skip if same hash already exists (DB enforces UNIQUE on member_hash per version)
            if all.iter().any(|(_, _, h, _)| h == mhash) {
                continue;
            }
            all.push((mtype.clone(), mref.clone(), mhash.clone(), next_ordinal));
            next_ordinal += 1;
        }

        // Compute collection hash and create version
        let hashes: Vec<&str> = all.iter().map(|(_, _, h, _)| h.as_str()).collect();
        let coll_hash = collection_hash(&hashes);

        let ver = sqlx::query_as::<_, CollectionVersion>(
            r#"
            INSERT INTO collection_versions (id, collection_id, version_number, hash, created_by)
            VALUES (
                $1, $2,
                COALESCE((SELECT MAX(version_number) FROM collection_versions WHERE collection_id = $2), 0) + 1,
                $3, $4
            )
            RETURNING *
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(collection_id)
        .bind(&coll_hash)
        .bind(created_by)
        .fetch_one(&mut *tx)
        .await?;

        let mut results = Vec::with_capacity(all.len());
        for (member_type, member_ref, member_hash, ordinal) in &all {
            let member = sqlx::query_as::<_, CollectionMember>(
                r#"
                INSERT INTO collection_members (id, collection_version_id, member_type, member_ref, member_hash, ordinal)
                VALUES ($1, $2, $3, $4, $5, $6)
                RETURNING *
                "#,
            )
            .bind(Uuid::new_v4())
            .bind(ver.id)
            .bind(member_type)
            .bind(member_ref)
            .bind(member_hash)
            .bind(ordinal)
            .fetch_one(&mut *tx)
            .await?;
            results.push(member);
        }

        tx.commit().await?;
        Ok(CollectionMutResult::Ok((ver, results)))
    }

    /// Remove members from a collection atomically.
    ///
    /// Acquires a project-level advisory lock, locks the collection row, re-checks
    /// yanked status, reads current members, filters out the specified refs,
    /// computes the new collection hash, and creates a new version.
    pub async fn remove_from_collection_atomically(
        &self,
        project_id: Uuid,
        collection_id: Uuid,
        created_by: Uuid,
        refs_to_remove: &[(String, String)], // (member_type, member_ref)
    ) -> Result<CollectionMutResult<Option<(CollectionVersion, Vec<CollectionMember>)>>> {
        let mut tx = self.pool.begin().await?;

        // Advisory lock serializes all collection mutations within a project
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1::text))")
            .bind(project_id)
            .execute(&mut *tx)
            .await?;

        // Lock collection row and re-check yanked status
        let coll =
            sqlx::query_as::<_, Collection>("SELECT * FROM collections WHERE id = $1 FOR UPDATE")
                .bind(collection_id)
                .fetch_one(&mut *tx)
                .await?;

        if coll.yanked {
            return Ok(CollectionMutResult::Yanked(coll.name));
        }

        // Read current members inside the lock
        let latest_ver = sqlx::query_as::<_, CollectionVersion>(
            "SELECT * FROM collection_versions WHERE collection_id = $1 ORDER BY version_number DESC LIMIT 1",
        )
        .bind(collection_id)
        .fetch_optional(&mut *tx)
        .await?;

        let Some(ref prev_ver) = latest_ver else {
            // No versions to remove from
            tx.commit().await?;
            return Ok(CollectionMutResult::Ok(None));
        };

        let current = sqlx::query_as::<_, CollectionMember>(
            "SELECT * FROM collection_members WHERE collection_version_id = $1 ORDER BY ordinal",
        )
        .bind(prev_ver.id)
        .fetch_all(&mut *tx)
        .await?;

        // Filter out removed members, re-number ordinals
        let remaining: Vec<(String, String, String, i32)> = current
            .iter()
            .filter(|m| {
                !refs_to_remove
                    .iter()
                    .any(|(t, r)| t == &m.member_type && r == &m.member_ref)
            })
            .enumerate()
            .map(|(i, m)| {
                (
                    m.member_type.clone(),
                    m.member_ref.clone(),
                    m.member_hash.clone(),
                    i as i32,
                )
            })
            .collect();

        // Compute hash and create version
        let hashes: Vec<&str> = remaining.iter().map(|(_, _, h, _)| h.as_str()).collect();
        let coll_hash = collection_hash(&hashes);

        let ver = sqlx::query_as::<_, CollectionVersion>(
            r#"
            INSERT INTO collection_versions (id, collection_id, version_number, hash, created_by)
            VALUES (
                $1, $2,
                COALESCE((SELECT MAX(version_number) FROM collection_versions WHERE collection_id = $2), 0) + 1,
                $3, $4
            )
            RETURNING *
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(collection_id)
        .bind(&coll_hash)
        .bind(created_by)
        .fetch_one(&mut *tx)
        .await?;

        let mut results = Vec::with_capacity(remaining.len());
        for (member_type, member_ref, member_hash, ordinal) in &remaining {
            let member = sqlx::query_as::<_, CollectionMember>(
                r#"
                INSERT INTO collection_members (id, collection_version_id, member_type, member_ref, member_hash, ordinal)
                VALUES ($1, $2, $3, $4, $5, $6)
                RETURNING *
                "#,
            )
            .bind(Uuid::new_v4())
            .bind(ver.id)
            .bind(member_type)
            .bind(member_ref)
            .bind(member_hash)
            .bind(ordinal)
            .fetch_one(&mut *tx)
            .await?;
            results.push(member);
        }

        tx.commit().await?;
        Ok(CollectionMutResult::Ok(Some((ver, results))))
    }

    /// DFS cycle detection: check if adding `target_name` as a sub-collection
    /// of `parent_name` would create a cycle.
    ///
    /// Uses the provided transaction connection for all reads, avoiding separate
    /// pool connections that could deadlock under contention when the advisory
    /// lock is held.
    async fn would_create_collection_cycle(
        tx: &mut sqlx::PgConnection,
        project_id: Uuid,
        parent_name: &str,
        target_name: &str,
    ) -> Result<bool> {
        if parent_name == target_name {
            return Ok(true);
        }

        let mut stack = vec![target_name.to_string()];
        let mut visited = std::collections::HashSet::new();

        while let Some(current) = stack.pop() {
            if !visited.insert(current.clone()) {
                continue;
            }

            let Some(coll) = sqlx::query_as::<_, Collection>(
                "SELECT * FROM collections WHERE project_id = $1 AND name = $2",
            )
            .bind(project_id)
            .bind(&current)
            .fetch_optional(&mut *tx)
            .await?
            else {
                continue;
            };

            let Some(ver) = sqlx::query_as::<_, CollectionVersion>(
                "SELECT * FROM collection_versions WHERE collection_id = $1 ORDER BY version_number DESC LIMIT 1",
            )
            .bind(coll.id)
            .fetch_optional(&mut *tx)
            .await?
            else {
                continue;
            };

            let members = sqlx::query_as::<_, CollectionMember>(
                "SELECT * FROM collection_members WHERE collection_version_id = $1 ORDER BY ordinal",
            )
            .bind(ver.id)
            .fetch_all(&mut *tx)
            .await?;

            for member in members {
                if member.member_type == "collection" {
                    if member.member_ref == parent_name {
                        return Ok(true);
                    }
                    stack.push(member.member_ref);
                }
            }
        }

        Ok(false)
    }

    pub async fn get_latest_collection_version(
        &self,
        collection_id: Uuid,
    ) -> Result<Option<CollectionVersion>> {
        let ver = sqlx::query_as::<_, CollectionVersion>(
            "SELECT * FROM collection_versions WHERE collection_id = $1 ORDER BY version_number DESC LIMIT 1",
        )
        .bind(collection_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(ver)
    }

    pub async fn list_collection_versions(
        &self,
        collection_id: Uuid,
    ) -> Result<Vec<CollectionVersion>> {
        let vers = sqlx::query_as::<_, CollectionVersion>(
            "SELECT * FROM collection_versions WHERE collection_id = $1 ORDER BY version_number DESC",
        )
        .bind(collection_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(vers)
    }

    pub async fn get_collection_members(&self, version_id: Uuid) -> Result<Vec<CollectionMember>> {
        let members = sqlx::query_as::<_, CollectionMember>(
            "SELECT * FROM collection_members WHERE collection_version_id = $1 ORDER BY ordinal",
        )
        .bind(version_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(members)
    }

    // ========================================================================
    // Endpoint Yank Operations
    // ========================================================================

    pub async fn insert_endpoint_yank(
        &self,
        project_id: Uuid,
        endpoint_name: &str,
        commit_id: Uuid,
        yank_reason: &str,
        yanked_by: Uuid,
    ) -> Result<EndpointYank> {
        let yank = sqlx::query_as::<_, EndpointYank>(
            r#"
            INSERT INTO endpoint_yanks (id, project_id, endpoint_name, commit_id, yank_reason, yanked_by)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING *
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(project_id)
        .bind(endpoint_name)
        .bind(commit_id)
        .bind(yank_reason)
        .bind(yanked_by)
        .fetch_one(&self.pool)
        .await?;
        Ok(yank)
    }

    pub async fn is_endpoint_yanked(
        &self,
        project_id: Uuid,
        endpoint_name: &str,
        commit_id: Uuid,
    ) -> Result<bool> {
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM endpoint_yanks WHERE project_id = $1 AND endpoint_name = $2 AND commit_id = $3)",
        )
        .bind(project_id)
        .bind(endpoint_name)
        .bind(commit_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(exists)
    }

    pub async fn list_endpoint_yanks(&self, project_id: Uuid) -> Result<Vec<EndpointYank>> {
        let yanks = sqlx::query_as::<_, EndpointYank>(
            "SELECT * FROM endpoint_yanks WHERE project_id = $1 ORDER BY yanked_at DESC",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(yanks)
    }

    pub async fn remove_endpoint_yank(
        &self,
        project_id: Uuid,
        endpoint_name: &str,
        commit_id: Uuid,
    ) -> Result<bool> {
        let result = sqlx::query(
            "DELETE FROM endpoint_yanks WHERE project_id = $1 AND endpoint_name = $2 AND commit_id = $3",
        )
        .bind(project_id)
        .bind(endpoint_name)
        .bind(commit_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    // ========================================================================
    // Secret Operations
    // ========================================================================

    /// Set (upsert) a secret. Generates a new version_id on every call.
    pub async fn upsert_secret(
        &self,
        project_id: Uuid,
        name: &str,
        encrypted_value: &[u8],
        set_by: Uuid,
    ) -> Result<Secret> {
        let secret = sqlx::query_as::<_, Secret>(
            r#"
            INSERT INTO secrets (id, project_id, name, encrypted_value, version_id, set_by)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (project_id, name) DO UPDATE SET
                encrypted_value = EXCLUDED.encrypted_value,
                version_id = EXCLUDED.version_id,
                set_by = EXCLUDED.set_by,
                updated_at = now()
            RETURNING *
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(project_id)
        .bind(name)
        .bind(encrypted_value)
        .bind(Uuid::new_v4()) // fresh version_id each time
        .bind(set_by)
        .fetch_one(&self.pool)
        .await?;
        Ok(secret)
    }

    /// List secret names and version IDs (no values).
    pub async fn list_secrets(&self, project_id: Uuid) -> Result<Vec<SecretInfo>> {
        let secrets = sqlx::query_as::<_, SecretInfo>(
            "SELECT id, name, version_id, created_at, updated_at FROM secrets WHERE project_id = $1 ORDER BY name",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(secrets)
    }

    /// Get a secret by name (includes encrypted value — for server-side decryption only).
    pub async fn get_secret(&self, project_id: Uuid, name: &str) -> Result<Option<Secret>> {
        let secret = sqlx::query_as::<_, Secret>(
            "SELECT * FROM secrets WHERE project_id = $1 AND name = $2",
        )
        .bind(project_id)
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;
        Ok(secret)
    }

    /// Get secret metadata by name (without the encrypted value).
    pub async fn get_secret_info(
        &self,
        project_id: Uuid,
        name: &str,
    ) -> Result<Option<SecretInfo>> {
        let info = sqlx::query_as::<_, SecretInfo>(
            "SELECT id, name, version_id, created_at, updated_at FROM secrets WHERE project_id = $1 AND name = $2",
        )
        .bind(project_id)
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;
        Ok(info)
    }

    pub async fn delete_secret(&self, project_id: Uuid, name: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM secrets WHERE project_id = $1 AND name = $2")
            .bind(project_id)
            .bind(name)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    // ========================================================================
    // Environment Image Operations
    // ========================================================================

    pub async fn insert_environment_image(
        &self,
        env_hash: &str,
        image_ref: &str,
        build_type: &str,
        base_image: Option<&str>,
    ) -> Result<EnvironmentImage> {
        let img = sqlx::query_as::<_, EnvironmentImage>(
            r#"
            INSERT INTO environment_images (id, env_hash, image_ref, build_type, base_image)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (env_hash) DO UPDATE SET env_hash = EXCLUDED.env_hash
            RETURNING *
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(env_hash)
        .bind(image_ref)
        .bind(build_type)
        .bind(base_image)
        .fetch_one(&self.pool)
        .await?;
        Ok(img)
    }

    pub async fn get_environment_image(&self, env_hash: &str) -> Result<Option<EnvironmentImage>> {
        let img = sqlx::query_as::<_, EnvironmentImage>(
            "SELECT * FROM environment_images WHERE env_hash = $1",
        )
        .bind(env_hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(img)
    }

    pub async fn mark_environment_built(
        &self,
        env_hash: &str,
        build_log_r2_key: Option<&str>,
        build_duration_ms: i32,
    ) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE environment_images SET built_at = now(), build_log_r2_key = $2, build_duration_ms = $3 WHERE env_hash = $1",
        )
        .bind(env_hash)
        .bind(build_log_r2_key)
        .bind(build_duration_ms)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Delete a pending (unbuilt) environment image row so it can be retried.
    /// Only deletes if `built_at IS NULL` to avoid removing a successfully built image.
    pub async fn delete_pending_environment_image(&self, env_hash: &str) -> Result<bool> {
        let result =
            sqlx::query("DELETE FROM environment_images WHERE env_hash = $1 AND built_at IS NULL")
                .bind(env_hash)
                .execute(&self.pool)
                .await?;
        Ok(result.rows_affected() > 0)
    }

    // ========================================================================
    // Source Cache Operations
    // ========================================================================

    pub async fn insert_source_cache(
        &self,
        git_provider: &str,
        git_repo: &str,
        git_commit_sha: &str,
        r2_key: &str,
        byte_size: i64,
    ) -> Result<SourceCacheEntry> {
        let entry = sqlx::query_as::<_, SourceCacheEntry>(
            r#"
            INSERT INTO source_cache (id, git_provider, git_repo, git_commit_sha, r2_key, byte_size)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (git_provider, git_repo, git_commit_sha) DO UPDATE SET
                last_accessed = now()
            RETURNING *
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(git_provider)
        .bind(git_repo)
        .bind(git_commit_sha)
        .bind(r2_key)
        .bind(byte_size)
        .fetch_one(&self.pool)
        .await?;
        Ok(entry)
    }

    pub async fn get_source_cache(
        &self,
        git_provider: &str,
        git_repo: &str,
        git_commit_sha: &str,
    ) -> Result<Option<SourceCacheEntry>> {
        let entry = sqlx::query_as::<_, SourceCacheEntry>(
            "SELECT * FROM source_cache WHERE git_provider = $1 AND git_repo = $2 AND git_commit_sha = $3",
        )
        .bind(git_provider)
        .bind(git_repo)
        .bind(git_commit_sha)
        .fetch_optional(&self.pool)
        .await?;
        Ok(entry)
    }

    pub async fn touch_source_cache(
        &self,
        git_provider: &str,
        git_repo: &str,
        git_commit_sha: &str,
    ) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE source_cache SET last_accessed = now() WHERE git_provider = $1 AND git_repo = $2 AND git_commit_sha = $3",
        )
        .bind(git_provider)
        .bind(git_repo)
        .bind(git_commit_sha)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    // ========================================================================
    // Materialized Cache Operations
    // ========================================================================

    pub async fn insert_materialized_cache(
        &self,
        materialized_hash: &str,
        project_id: Uuid,
        commit_id: Uuid,
        endpoint_name: &str,
        node_name: &str,
        transform_name: &str,
        output_hash: &str,
        output_r2_key: &str,
        output_content_type: &str,
        output_byte_size: i64,
        platform: &str,
        verification_tier: i32,
    ) -> Result<MaterializedCacheEntry> {
        let entry = sqlx::query_as::<_, MaterializedCacheEntry>(
            r#"
            INSERT INTO materialized_cache (
                materialized_hash, project_id, commit_id, endpoint_name, node_name,
                transform_name, output_hash, output_r2_key, output_content_type,
                output_byte_size, platform, verification_tier
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            ON CONFLICT (materialized_hash) DO UPDATE SET
                last_accessed = now(),
                access_count = materialized_cache.access_count + 1
            RETURNING *
            "#,
        )
        .bind(materialized_hash)
        .bind(project_id)
        .bind(commit_id)
        .bind(endpoint_name)
        .bind(node_name)
        .bind(transform_name)
        .bind(output_hash)
        .bind(output_r2_key)
        .bind(output_content_type)
        .bind(output_byte_size)
        .bind(platform)
        .bind(verification_tier)
        .fetch_one(&self.pool)
        .await?;
        Ok(entry)
    }

    pub async fn get_materialized_cache(
        &self,
        materialized_hash: &str,
    ) -> Result<Option<MaterializedCacheEntry>> {
        let entry = sqlx::query_as::<_, MaterializedCacheEntry>(
            "SELECT * FROM materialized_cache WHERE materialized_hash = $1",
        )
        .bind(materialized_hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(entry)
    }

    /// Touch the cache entry to update access tracking. Returns true if found.
    pub async fn touch_materialized_cache(&self, materialized_hash: &str) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE materialized_cache SET last_accessed = now(), access_count = access_count + 1 WHERE materialized_hash = $1",
        )
        .bind(materialized_hash)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn list_materialized_cache(
        &self,
        project_id: Uuid,
        endpoint_name: Option<&str>,
    ) -> Result<Vec<MaterializedCacheEntry>> {
        match endpoint_name {
            Some(name) => {
                let entries = sqlx::query_as::<_, MaterializedCacheEntry>(
                    "SELECT * FROM materialized_cache WHERE project_id = $1 AND endpoint_name = $2 ORDER BY last_accessed DESC",
                )
                .bind(project_id)
                .bind(name)
                .fetch_all(&self.pool)
                .await?;
                Ok(entries)
            }
            None => {
                let entries = sqlx::query_as::<_, MaterializedCacheEntry>(
                    "SELECT * FROM materialized_cache WHERE project_id = $1 ORDER BY last_accessed DESC",
                )
                .bind(project_id)
                .fetch_all(&self.pool)
                .await?;
                Ok(entries)
            }
        }
    }

    pub async fn delete_materialized_cache(&self, materialized_hash: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM materialized_cache WHERE materialized_hash = $1")
            .bind(materialized_hash)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    // ========================================================================
    // Job Operations
    // ========================================================================

    /// Create a new job. Returns the created job.
    pub async fn create_job(
        &self,
        project_id: Uuid,
        endpoint_name: &str,
        commit_id: Uuid,
        params: &serde_json::Value,
        params_hash: &str,
        node_status: &serde_json::Value,
        created_by: Option<Uuid>,
    ) -> Result<Job> {
        let job = sqlx::query_as::<_, Job>(
            "INSERT INTO jobs (project_id, endpoint_name, commit_id, params, params_hash, node_status, created_by, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, now() + interval '24 hours')
             RETURNING *",
        )
        .bind(project_id)
        .bind(endpoint_name)
        .bind(commit_id)
        .bind(params)
        .bind(params_hash)
        .bind(node_status)
        .bind(created_by)
        .fetch_one(&self.pool)
        .await?;
        Ok(job)
    }

    /// Get a job by ID.
    pub async fn get_job(&self, job_id: Uuid) -> Result<Option<Job>> {
        let job = sqlx::query_as::<_, Job>("SELECT * FROM jobs WHERE id = $1")
            .bind(job_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(job)
    }

    /// Find an active (queued or running) job with the same dedup key.
    pub async fn find_active_job(
        &self,
        project_id: Uuid,
        endpoint_name: &str,
        commit_id: Uuid,
        params_hash: &str,
    ) -> Result<Option<Job>> {
        let job = sqlx::query_as::<_, Job>(
            "SELECT * FROM jobs
             WHERE project_id = $1 AND endpoint_name = $2 AND commit_id = $3 AND params_hash = $4
             AND status IN ('queued', 'running')
             ORDER BY created_at DESC
             LIMIT 1",
        )
        .bind(project_id)
        .bind(endpoint_name)
        .bind(commit_id)
        .bind(params_hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(job)
    }

    /// Update job status. Sets started_at when transitioning to 'running',
    /// completed_at when transitioning to 'done' or 'failed'.
    pub async fn update_job_status(&self, job_id: Uuid, status: &str) -> Result<bool> {
        let result = match status {
            "running" => {
                sqlx::query(
                    "UPDATE jobs SET status = $2, started_at = now() WHERE id = $1 AND status IN ('queued', 'running')",
                )
                .bind(job_id)
                .bind(status)
                .execute(&self.pool)
                .await?
            }
            "done" | "failed" => {
                sqlx::query(
                    "UPDATE jobs SET status = $2, completed_at = now() WHERE id = $1",
                )
                .bind(job_id)
                .bind(status)
                .execute(&self.pool)
                .await?
            }
            _ => {
                sqlx::query("UPDATE jobs SET status = $2 WHERE id = $1")
                    .bind(job_id)
                    .bind(status)
                    .execute(&self.pool)
                    .await?
            }
        };
        Ok(result.rows_affected() > 0)
    }

    /// Update individual node status in the JSONB node_status field.
    pub async fn update_node_status(
        &self,
        job_id: Uuid,
        node_name: &str,
        status: &str,
    ) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE jobs SET node_status = jsonb_set(node_status, ARRAY[$2], to_jsonb($3::text))
             WHERE id = $1",
        )
        .bind(job_id)
        .bind(node_name)
        .bind(status)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Set job output on completion.
    pub async fn set_job_output(
        &self,
        job_id: Uuid,
        output_hash: &str,
        output_content_type: &str,
    ) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE jobs SET output_hash = $2, output_content_type = $3,
             status = 'done', completed_at = now()
             WHERE id = $1",
        )
        .bind(job_id)
        .bind(output_hash)
        .bind(output_content_type)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Set job error on failure.
    pub async fn set_job_error(&self, job_id: Uuid, error_message: &str) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE jobs SET error_message = $2, status = 'failed', completed_at = now()
             WHERE id = $1",
        )
        .bind(job_id)
        .bind(error_message)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// List jobs for a project, ordered by most recent first.
    pub async fn list_jobs(&self, project_id: Uuid, limit: i64) -> Result<Vec<Job>> {
        let jobs = sqlx::query_as::<_, Job>(
            "SELECT * FROM jobs WHERE project_id = $1 ORDER BY created_at DESC LIMIT $2",
        )
        .bind(project_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(jobs)
    }

    /// Delete expired jobs.
    pub async fn cleanup_expired_jobs(&self) -> Result<u64> {
        let result =
            sqlx::query("DELETE FROM jobs WHERE expires_at IS NOT NULL AND expires_at < now()")
                .execute(&self.pool)
                .await?;
        Ok(result.rows_affected())
    }

    /// Count active (queued or running) jobs for a specific user.
    pub async fn count_active_jobs_for_user(&self, user_id: Uuid) -> Result<i64> {
        let row = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM jobs WHERE created_by = $1 AND status IN ('queued', 'running')",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    /// Count active (queued or running) jobs globally across all users.
    pub async fn count_active_jobs_global(&self) -> Result<i64> {
        let row = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM jobs WHERE status IN ('queued', 'running')",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    // ========================================================================
    // Environment Provider Images
    // ========================================================================

    /// Get the image reference for an environment on a specific provider.
    pub async fn get_provider_image(
        &self,
        env_hash: &str,
        provider: &str,
    ) -> Result<Option<EnvironmentProviderImage>> {
        let image = sqlx::query_as::<_, EnvironmentProviderImage>(
            "SELECT * FROM environment_provider_images WHERE env_hash = $1 AND provider = $2",
        )
        .bind(env_hash)
        .bind(provider)
        .fetch_optional(&self.pool)
        .await?;
        Ok(image)
    }

    /// Upsert an environment provider image record.
    pub async fn upsert_provider_image(
        &self,
        env_hash: &str,
        provider: &str,
        image_ref: &str,
    ) -> Result<EnvironmentProviderImage> {
        let image = sqlx::query_as::<_, EnvironmentProviderImage>(
            "INSERT INTO environment_provider_images (env_hash, provider, image_ref)
             VALUES ($1, $2, $3)
             ON CONFLICT (env_hash, provider) DO UPDATE SET image_ref = $3, pushed_at = now()
             RETURNING *",
        )
        .bind(env_hash)
        .bind(provider)
        .bind(image_ref)
        .fetch_one(&self.pool)
        .await?;
        Ok(image)
    }
}
