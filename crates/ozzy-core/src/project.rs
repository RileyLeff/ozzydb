//! Project management - ozzy.toml and .ozzy/ directory.
//!
//! A project directory contains:
//! - ozzy.toml: Project metadata and current HEAD
//! - .ozzy/: Internal directory with commits, refs, and objects
//! - data/: Working directory for staged data
//! - transforms/: Working directory for staged transforms

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Project configuration stored in ozzy.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub project: ProjectMetadata,

    #[serde(default)]
    pub refs: RefsConfig,

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
    pub hash: String,
    pub runtime: String,         // e.g., "python-3.11"
    pub source_path: String,     // Relative path in project
    pub function_name: String,   // Function to call
    pub lockfile_hash: String,
    pub params_schema: serde_json::Value,

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
    pub data_sources: HashMap<String, DataSource>,
    pub transforms: HashMap<String, Transform>,
    pub endpoints: HashMap<String, Endpoint>,
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
            workspace: WorkspaceConfig::default(),
        };

        let config_content = toml::to_string_pretty(&config)?;
        fs::write(&config_path, config_content)?;

        // Create .gitignore for .ozzy directory internals
        let gitignore_content = "# OzzyDB internals\n.ozzy/objects/\n.ozzy/commits/\n";
        fs::write(root.join(".ozzy/.gitignore"), gitignore_content)?;

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

    // ─────────────────────────────────────────────────────────────────────
    // Ref operations
    // ─────────────────────────────────────────────────────────────────────

    /// Get the current HEAD commit hash.
    pub fn head_commit(&self) -> Result<Option<String>> {
        self.resolve_ref(&self.config.refs.head)
    }

    /// Resolve a ref to a commit hash.
    pub fn resolve_ref(&self, ref_name: &str) -> Result<Option<String>> {
        let ref_path = self.refs_dir().join(ref_name.strip_prefix("refs/").unwrap_or(ref_name));
        if ref_path.exists() {
            let hash = fs::read_to_string(&ref_path)?.trim().to_string();
            Ok(Some(hash))
        } else {
            Ok(None)
        }
    }

    /// Update a ref to point to a commit.
    pub fn update_ref(&self, ref_name: &str, commit_hash: &str) -> Result<()> {
        let ref_path = self.refs_dir().join(ref_name.strip_prefix("refs/").unwrap_or(ref_name));
        if let Some(parent) = ref_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(ref_path, format!("{}\n", commit_hash))?;
        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────
    // Commit operations
    // ─────────────────────────────────────────────────────────────────────

    /// Load a commit by hash.
    pub fn load_commit(&self, hash: &str) -> Result<Commit> {
        let commit_path = self.commits_dir().join(format!("{}.json", hash));
        if !commit_path.exists() {
            return Err(Error::CommitNotFound(hash.to_string()));
        }
        let content = fs::read_to_string(&commit_path)?;
        let commit: Commit = serde_json::from_str(&content)?;
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
}
