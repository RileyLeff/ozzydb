//! Database query implementations for v2 schema.

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
            INSERT INTO api_tokens (id, user_id, name, token_hash, scope, expires_at)
            VALUES ($1, $2, $3, $4, 'account', $5)
            ON CONFLICT (user_id, name) DO UPDATE SET
                token_hash = EXCLUDED.token_hash,
                scope = EXCLUDED.scope,
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

    pub async fn get_commit_by_sha(
        &self,
        project_id: Uuid,
        sha: &str,
    ) -> Result<Option<Commit>> {
        let commit = sqlx::query_as::<_, Commit>(
            "SELECT * FROM commits WHERE project_id = $1 AND git_commit_sha = $2",
        )
        .bind(project_id)
        .bind(sha)
        .fetch_optional(&self.pool)
        .await?;
        Ok(commit)
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

    pub async fn resolve_ref(
        &self,
        project_id: Uuid,
        ref_name: &str,
    ) -> Result<Option<Ref>> {
        let r = sqlx::query_as::<_, Ref>(
            "SELECT * FROM refs WHERE project_id = $1 AND ref_name = $2",
        )
        .bind(project_id)
        .bind(ref_name)
        .fetch_optional(&self.pool)
        .await?;
        Ok(r)
    }

    pub async fn list_refs(&self, project_id: Uuid) -> Result<Vec<Ref>> {
        let refs = sqlx::query_as::<_, Ref>(
            "SELECT * FROM refs WHERE project_id = $1 ORDER BY ref_name",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(refs)
    }

    pub async fn delete_ref(&self, project_id: Uuid, ref_name: &str) -> Result<bool> {
        let result =
            sqlx::query("DELETE FROM refs WHERE project_id = $1 AND ref_name = $2")
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

    pub async fn get_data_atom(
        &self,
        project_id: Uuid,
        name: &str,
    ) -> Result<Option<DataAtom>> {
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

    pub async fn yank_data_atom(
        &self,
        project_id: Uuid,
        name: &str,
        reason: &str,
    ) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE data_atoms SET yanked = true, yank_reason = $3, yanked_at = now() WHERE project_id = $1 AND name = $2",
        )
        .bind(project_id)
        .bind(name)
        .bind(reason)
        .execute(&self.pool)
        .await?;
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
        let result =
            sqlx::query("DELETE FROM github_installations WHERE installation_id = $1")
                .bind(installation_id)
                .execute(&self.pool)
                .await?;
        Ok(result.rows_affected() > 0)
    }
}
