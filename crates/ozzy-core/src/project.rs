//! Project management - ozzy.toml and .ozzy/ directory.
//!
//! A project directory contains:
//! - ozzy.toml: Project metadata and current HEAD
//! - .ozzy/: Internal directory with commits, refs, and objects
//! - data/: Working directory for staged data
//! - transforms/: Working directory for staged transforms

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::error::{Error, Result};

/// Validate that a name is safe for use in paths (no path traversal).
/// Names should be simple identifiers without slashes, dots at the start, or other special chars.
pub fn validate_safe_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(Error::InvalidPath("Name cannot be empty".to_string()));
    }

    // Block leading dots (hidden files / parent traversal)
    if name.starts_with('.') {
        return Err(Error::InvalidPath(format!(
            "Name cannot start with a dot: {}",
            name
        )));
    }

    // Allow alphanumeric, underscore, hyphen, and dot (for semver tags like v1.0.0)
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        return Err(Error::InvalidPath(format!(
            "Name must contain only [a-zA-Z0-9._-]: {}",
            name
        )));
    }

    Ok(())
}

/// Validate that a path stays within a base directory after canonicalization.
/// This prevents symlink attacks and path traversal via .. components.
pub fn validate_path_within(path: &Path, base_dir: &Path) -> Result<()> {
    // Canonicalize both paths to resolve symlinks and .. components
    let canonical_base = base_dir.canonicalize().map_err(|e| {
        Error::InvalidPath(format!(
            "Cannot canonicalize base directory {}: {}",
            base_dir.display(),
            e
        ))
    })?;

    let canonical_path = path.canonicalize().map_err(|e| {
        Error::InvalidPath(format!(
            "Cannot canonicalize path {}: {}",
            path.display(),
            e
        ))
    })?;

    // Check that the canonical path starts with the canonical base
    if !canonical_path.starts_with(&canonical_base) {
        return Err(Error::InvalidPath(format!(
            "Path {} escapes base directory {}",
            path.display(),
            base_dir.display()
        )));
    }

    Ok(())
}

/// Project configuration stored in ozzy.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub project: ProjectMetadata,

    #[serde(default)]
    pub refs: RefsConfig,

    /// Named remote registries.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub remotes: BTreeMap<String, String>,

    #[serde(default)]
    pub workspace: WorkspaceConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMetadata {
    pub name: String,
    pub owner: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(default = "default_version")]
    pub version: String,
}

fn default_version() -> String {
    "0.1.0".to_string()
}

fn ensure_root_gitignore(root: &Path) -> Result<()> {
    let gitignore_path = root.join(".gitignore");
    let required_entries = [".ozzy/", "data/*.parquet"];

    let mut content = if gitignore_path.exists() {
        fs::read_to_string(&gitignore_path)?
    } else {
        String::new()
    };

    let existing: HashSet<String> = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| line.to_string())
        .collect();

    let missing: Vec<&str> = required_entries
        .iter()
        .copied()
        .filter(|entry| !existing.contains(*entry))
        .collect();

    if missing.is_empty() {
        return Ok(());
    }

    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    if content.is_empty() {
        content.push_str("# OzzyDB generated\n");
    }
    for entry in missing {
        content.push_str(entry);
        content.push('\n');
    }

    fs::write(gitignore_path, content)?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RefsConfig {
    /// Current HEAD reference (e.g., "refs/heads/main")
    #[serde(default = "default_head")]
    pub head: String,

    /// Remote registry URL (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
}

fn default_head() -> String {
    "refs/heads/main".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkspaceConfig {
    /// Staged data files
    #[serde(default)]
    pub staged_data: Vec<String>,

    /// Staged transform files
    #[serde(default)]
    pub staged_transforms: Vec<String>,
}

/// Data source metadata
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DataSource {
    pub name: String,
    pub hash: String,
    pub schema_hash: String,
    pub path: String, // Relative path in project
    pub row_count: Option<u64>,
    pub byte_size: Option<u64>,
}

/// Transform metadata
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Transform {
    pub name: String,
    pub hash: String,          // Composite: blake3(source_hash + function_name + lockfile + runtime + params_schema)
    pub runtime: String,       // e.g., "python-3.11"
    pub source_path: String,   // Relative path in project
    pub function_name: String, // Function to call
    pub lockfile_hash: String,
    pub params_schema: serde_json::Value,

    /// Content hash of the canonical source file: blake3(canonicalize_source(text)).
    /// Used for content-addressed storage lookups and integrity verification.
    /// Distinct from `hash` which is the composite identity hash for caching.
    #[serde(default)]
    pub source_hash: String,

    #[serde(default = "default_reproducible")]
    pub reproducible: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<serde_json::Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<serde_json::Value>,
}

fn default_reproducible() -> bool {
    true
}

/// Pipeline node in an endpoint's DAG
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PipelineNode {
    pub node_name: String,
    pub transform_name: String,
    pub params: serde_json::Value,
}

/// Pipeline edge representing data flow
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PipelineEdge {
    pub target_node: String,
    pub input_name: String,
    pub source_type: SourceType,
    pub source_ref: String,

    // For external dependencies
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_project: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_commit_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    DataSource,
    Node,
    External,
}

/// Endpoint definition
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Endpoint {
    pub name: String,
    pub nodes: Vec<PipelineNode>,
    pub edges: Vec<PipelineEdge>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Commit object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Commit {
    pub hash: String,
    pub parent_hashes: Vec<String>,
    pub author: String,
    pub message: String,
    pub timestamp: DateTime<Utc>,
    /// Data sources (BTreeMap for deterministic serialization order)
    pub data_sources: BTreeMap<String, DataSource>,
    /// Transforms (BTreeMap for deterministic serialization order)
    pub transforms: BTreeMap<String, Transform>,
    /// Endpoints (BTreeMap for deterministic serialization order)
    pub endpoints: BTreeMap<String, Endpoint>,
}

/// Project handle for interacting with an OzzyDB project
pub struct Project {
    /// Root directory of the project
    pub root: PathBuf,

    /// Project configuration
    pub config: ProjectConfig,
}

impl Project {
    /// Find and open a project in the current directory or parents.
    pub fn find_current() -> Result<Self> {
        let current = std::env::current_dir()?;
        Self::find_in(&current)
    }

    /// Find a project starting from the given directory.
    pub fn find_in(start: &Path) -> Result<Self> {
        let mut dir = start.to_path_buf();
        loop {
            let config_path = dir.join("ozzy.toml");
            if config_path.exists() {
                return Self::open(&dir);
            }
            if !dir.pop() {
                return Err(Error::NotInProject);
            }
        }
    }

    /// Open an existing project at the given path.
    pub fn open(root: &Path) -> Result<Self> {
        let config_path = root.join("ozzy.toml");
        if !config_path.exists() {
            return Err(Error::NotInProject);
        }

        let config_content = fs::read_to_string(&config_path)?;
        let config: ProjectConfig = toml::from_str(&config_content)?;

        Ok(Self {
            root: root.to_path_buf(),
            config,
        })
    }

    /// Initialize a new project.
    pub fn init(root: &Path, name: &str, owner: &str) -> Result<Self> {
        let config_path = root.join("ozzy.toml");
        if config_path.exists() {
            return Err(Error::ProjectAlreadyExists);
        }

        // Create directory structure
        fs::create_dir_all(root)?;
        fs::create_dir_all(root.join(".ozzy/commits"))?;
        fs::create_dir_all(root.join(".ozzy/refs/heads"))?;
        fs::create_dir_all(root.join(".ozzy/refs/tags"))?;
        fs::create_dir_all(root.join(".ozzy/objects/data"))?;
        fs::create_dir_all(root.join(".ozzy/objects/transforms"))?;
        fs::create_dir_all(root.join("data"))?;
        fs::create_dir_all(root.join("transforms"))?;

        // Create initial config
        let config = ProjectConfig {
            project: ProjectMetadata {
                name: name.to_string(),
                owner: owner.to_string(),
                description: None,
                version: default_version(),
            },
            refs: RefsConfig {
                head: default_head(),
                remote: None,
            },
            remotes: BTreeMap::new(),
            workspace: WorkspaceConfig::default(),
        };

        let config_content = toml::to_string_pretty(&config)?;
        fs::write(&config_path, config_content)?;

        // Ensure git ignores local internals and raw parquet inputs.
        ensure_root_gitignore(root)?;

        Ok(Self {
            root: root.to_path_buf(),
            config,
        })
    }

    /// Save the project config to disk.
    pub fn save_config(&self) -> Result<()> {
        let config_path = self.root.join("ozzy.toml");
        let config_content = toml::to_string_pretty(&self.config)?;
        fs::write(config_path, config_content)?;
        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────
    // Path helpers
    // ─────────────────────────────────────────────────────────────────────

    pub fn ozzy_dir(&self) -> PathBuf {
        self.root.join(".ozzy")
    }

    pub fn commits_dir(&self) -> PathBuf {
        self.ozzy_dir().join("commits")
    }

    pub fn refs_dir(&self) -> PathBuf {
        self.ozzy_dir().join("refs")
    }

    pub fn objects_dir(&self) -> PathBuf {
        self.ozzy_dir().join("objects")
    }

    pub fn data_dir(&self) -> PathBuf {
        self.root.join("data")
    }

    pub fn transforms_dir(&self) -> PathBuf {
        self.root.join("transforms")
    }

    fn normalize_ref_relative_path(ref_name: &str) -> Result<PathBuf> {
        let relative = ref_name.strip_prefix("refs/").unwrap_or(ref_name);
        if relative.is_empty() {
            return Err(Error::InvalidPath("Ref name cannot be empty".to_string()));
        }

        let relative_path = Path::new(relative);
        if relative_path.is_absolute() {
            return Err(Error::InvalidPath(format!(
                "Ref path must be relative: {}",
                ref_name
            )));
        }

        let mut sanitized = PathBuf::new();
        for component in relative_path.components() {
            match component {
                Component::Normal(part) => {
                    let part_str = part.to_str().ok_or_else(|| {
                        Error::InvalidPath(format!(
                            "Ref contains non-UTF-8 path component: {}",
                            ref_name
                        ))
                    })?;
                    validate_safe_name(part_str)?;
                    sanitized.push(part);
                }
                _ => {
                    return Err(Error::InvalidPath(format!(
                        "Invalid ref path '{}': path traversal or absolute components are not allowed",
                        ref_name
                    )));
                }
            }
        }

        if sanitized.as_os_str().is_empty() {
            return Err(Error::InvalidPath("Ref name cannot be empty".to_string()));
        }

        Ok(sanitized)
    }

    fn assert_ref_path_within_refs_dir(&self, ref_path: &Path) -> Result<()> {
        let refs_dir = self.refs_dir();
        let canonical_refs = refs_dir.canonicalize().map_err(|e| {
            Error::InvalidPath(format!(
                "Cannot canonicalize refs directory {}: {}",
                refs_dir.display(),
                e
            ))
        })?;

        if ref_path.exists() {
            let canonical_ref_path = ref_path.canonicalize().map_err(|e| {
                Error::InvalidPath(format!(
                    "Cannot canonicalize ref path {}: {}",
                    ref_path.display(),
                    e
                ))
            })?;
            if !canonical_ref_path.starts_with(&canonical_refs) {
                return Err(Error::InvalidPath(format!(
                    "Ref path {} escapes refs directory {}",
                    ref_path.display(),
                    refs_dir.display()
                )));
            }
            return Ok(());
        }

        let mut current = ref_path.parent().ok_or_else(|| {
            Error::InvalidPath(format!(
                "Ref path has no parent directory: {}",
                ref_path.display()
            ))
        })?;

        loop {
            if current.exists() {
                let canonical_current = current.canonicalize().map_err(|e| {
                    Error::InvalidPath(format!(
                        "Cannot canonicalize ancestor path {}: {}",
                        current.display(),
                        e
                    ))
                })?;
                if !canonical_current.starts_with(&canonical_refs) {
                    return Err(Error::InvalidPath(format!(
                        "Ref path {} escapes refs directory {}",
                        ref_path.display(),
                        refs_dir.display()
                    )));
                }
                return Ok(());
            }

            current = current.parent().ok_or_else(|| {
                Error::InvalidPath(format!(
                    "Ref path {} does not have an ancestor within refs directory",
                    ref_path.display()
                ))
            })?;
        }
    }

    fn checked_ref_path(&self, ref_name: &str) -> Result<PathBuf> {
        let relative = Self::normalize_ref_relative_path(ref_name)?;
        let ref_path = self.refs_dir().join(relative);
        self.assert_ref_path_within_refs_dir(&ref_path)?;
        Ok(ref_path)
    }

    // ─────────────────────────────────────────────────────────────────────
    // Ref operations
    // ─────────────────────────────────────────────────────────────────────

    /// Get the current HEAD commit hash.
    pub fn head_commit(&self) -> Result<Option<String>> {
        self.resolve_ref(&self.config.refs.head)
    }

    /// Resolve a ref to a commit hash.
    ///
    /// Supports multiple formats:
    /// - `refs/heads/main` or `refs/tags/v1.0` — direct ref path lookup
    /// - `@latest` — resolves to the default branch (refs/heads/main)
    /// - `@v1.0.0` — resolves to a tag (refs/tags/v1.0.0)
    /// - 64-char hex string — treated as a raw commit hash
    pub fn resolve_ref(&self, ref_name: &str) -> Result<Option<String>> {
        // Handle @latest → default branch
        if ref_name == "@latest" {
            let default_branch = &self.config.refs.head;
            let ref_path = self.checked_ref_path(default_branch)?;
            if ref_path.exists() {
                let hash = fs::read_to_string(&ref_path)?.trim().to_string();
                return Ok(Some(hash));
            }
            return Ok(None);
        }

        // Handle @tag_name → refs/tags/tag_name
        if let Some(tag_name) = ref_name.strip_prefix('@') {
            let tag_path = self.checked_ref_path(&format!("refs/tags/{}", tag_name))?;
            if tag_path.exists() {
                let hash = fs::read_to_string(&tag_path)?.trim().to_string();
                return Ok(Some(hash));
            }
            return Ok(None);
        }

        // Handle raw commit hash (64-char hex string)
        if ref_name.len() == 64 && ref_name.chars().all(|c| c.is_ascii_hexdigit()) {
            let commit_path = self.commits_dir().join(format!("{}.json", ref_name));
            if commit_path.exists() {
                return Ok(Some(ref_name.to_string()));
            }
            return Ok(None);
        }

        // Standard ref path lookup
        let ref_path = self.checked_ref_path(ref_name)?;
        if ref_path.exists() {
            let hash = fs::read_to_string(&ref_path)?.trim().to_string();
            Ok(Some(hash))
        } else {
            Ok(None)
        }
    }

    /// Update a ref to point to a commit.
    pub fn update_ref(&self, ref_name: &str, commit_hash: &str) -> Result<()> {
        let refs_dir = self.refs_dir();
        fs::create_dir_all(&refs_dir)?;

        let ref_path = self.checked_ref_path(ref_name)?;
        if let Some(parent) = ref_path.parent() {
            fs::create_dir_all(parent)?;
        }
        self.assert_ref_path_within_refs_dir(&ref_path)?;
        fs::write(ref_path, format!("{}\n", commit_hash))?;
        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────
    // Commit operations
    // ─────────────────────────────────────────────────────────────────────

    /// Load a commit by hash, verifying its integrity.
    pub fn load_commit(&self, hash: &str) -> Result<Commit> {
        let commit_path = self.commits_dir().join(format!("{}.json", hash));
        if !commit_path.exists() {
            return Err(Error::CommitNotFound(hash.to_string()));
        }
        let content = fs::read_to_string(&commit_path)?;
        let commit: Commit = serde_json::from_str(&content)?;

        let expected = crate::commit::compute_commit_hash(&commit);
        if expected != commit.hash {
            return Err(Error::InvalidPath(format!(
                "Commit hash mismatch: file says {}, computed {}. Commit may be corrupted.",
                commit.hash, expected
            )));
        }

        Ok(commit)
    }

    /// Save a commit to disk.
    pub fn save_commit(&self, commit: &Commit) -> Result<()> {
        let commit_path = self.commits_dir().join(format!("{}.json", commit.hash));
        let content = serde_json::to_string_pretty(commit)?;
        fs::write(commit_path, content)?;
        Ok(())
    }

    /// Get the latest commit (from HEAD).
    pub fn latest_commit(&self) -> Result<Option<Commit>> {
        if let Some(hash) = self.head_commit()? {
            Ok(Some(self.load_commit(&hash)?))
        } else {
            Ok(None)
        }
    }

    /// List all commits in reverse chronological order.
    pub fn list_commits(&self, limit: usize) -> Result<Vec<Commit>> {
        let mut commits = Vec::new();

        // Start from HEAD and follow parent chain
        if let Some(mut current_hash) = self.head_commit()? {
            while commits.len() < limit {
                let commit = self.load_commit(&current_hash)?;
                let parent = commit.parent_hashes.first().cloned();
                commits.push(commit);

                if let Some(parent_hash) = parent {
                    current_hash = parent_hash;
                } else {
                    break;
                }
            }
        }

        Ok(commits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_init_project() {
        let dir = tempdir().unwrap();
        let project = Project::init(dir.path(), "test-project", "testuser").unwrap();

        assert_eq!(project.config.project.name, "test-project");
        assert_eq!(project.config.project.owner, "testuser");
        assert!(dir.path().join("ozzy.toml").exists());
        assert!(dir.path().join(".ozzy/commits").exists());
        assert!(dir.path().join(".ozzy/refs/heads").exists());
        assert!(dir.path().join("data").exists());
        assert!(dir.path().join("transforms").exists());
        let gitignore = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(gitignore.contains(".ozzy/"));
        assert!(gitignore.contains("data/*.parquet"));
    }

    #[test]
    fn test_open_project() {
        let dir = tempdir().unwrap();
        Project::init(dir.path(), "test-project", "testuser").unwrap();

        let project = Project::open(dir.path()).unwrap();
        assert_eq!(project.config.project.name, "test-project");
    }

    #[test]
    fn test_find_project() {
        let dir = tempdir().unwrap();
        let subdir = dir.path().join("nested/deep");
        fs::create_dir_all(&subdir).unwrap();

        Project::init(dir.path(), "test-project", "testuser").unwrap();

        let project = Project::find_in(&subdir).unwrap();
        assert_eq!(project.config.project.name, "test-project");
    }

    #[test]
    fn test_not_in_project() {
        let dir = tempdir().unwrap();
        let result = Project::find_in(dir.path());
        assert!(matches!(result, Err(Error::NotInProject)));
    }

    #[test]
    fn test_validate_safe_name_strict_ascii_pattern() {
        assert!(validate_safe_name("main").is_ok());
        assert!(validate_safe_name("branch_1-test").is_ok());
        assert!(validate_safe_name("v1.0.0").is_ok());
        assert!(validate_safe_name(".hidden").is_err());
        assert!(validate_safe_name("has space").is_err());
        assert!(validate_safe_name("../escape").is_err());
    }

    #[test]
    fn test_update_ref_rejects_path_traversal() {
        let dir = tempdir().unwrap();
        let project = Project::init(dir.path(), "test-project", "testuser").unwrap();
        let result = project.update_ref("../../outside", "abc123");
        assert!(matches!(result, Err(Error::InvalidPath(_))));
    }

    #[test]
    fn test_resolve_ref_rejects_path_traversal() {
        let dir = tempdir().unwrap();
        let project = Project::init(dir.path(), "test-project", "testuser").unwrap();
        let result = project.resolve_ref("../../outside");
        assert!(matches!(result, Err(Error::InvalidPath(_))));
    }

    #[test]
    fn test_load_commit_rejects_corrupted_hash() {
        let dir = tempdir().unwrap();
        let project = Project::init(dir.path(), "test-project", "testuser").unwrap();

        // Create a commit via the normal path
        let commit = crate::commit::create_commit(&project, "test commit", "testuser").unwrap();
        project.save_commit(&commit).unwrap();

        // Tamper with the commit file to corrupt it
        let commit_path = project
            .commits_dir()
            .join(format!("{}.json", commit.hash));
        let mut content: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&commit_path).unwrap()).unwrap();
        content["message"] = serde_json::Value::String("tampered message".to_string());
        fs::write(&commit_path, serde_json::to_string_pretty(&content).unwrap()).unwrap();

        // Loading should fail with hash mismatch
        let result = project.load_commit(&commit.hash);
        assert!(result.is_err(), "load_commit should reject corrupted commit");
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("hash mismatch"),
            "Error should mention hash mismatch, got: {}",
            err_msg
        );
    }
}
