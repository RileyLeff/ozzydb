//! Runtime execution for Python transforms.
//!
//! Executes transforms in isolated environments with deterministic settings.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{Error, Result};
use crate::platform::PlatformFingerprint;

/// Deterministic environment variables for transform execution.
pub const DETERMINISTIC_ENV: &[(&str, &str)] = &[
    ("PYTHONHASHSEED", "0"),
    ("OMP_NUM_THREADS", "1"),
    ("MKL_NUM_THREADS", "1"),
    ("OPENBLAS_NUM_THREADS", "1"),
    ("NUMEXPR_NUM_THREADS", "1"),
];

/// Python runtime for executing transforms.
pub struct PythonRuntime {
    /// Path to uv executable
    uv_path: PathBuf,

    /// Base directory for virtual environments
    envs_dir: PathBuf,

    /// Current platform fingerprint
    platform: PlatformFingerprint,
}

impl PythonRuntime {
    /// Create a new Python runtime.
    pub fn new() -> Result<Self> {
        let uv_path = which::which("uv")
            .map_err(|_| Error::RuntimeError("uv not found in PATH. Install with: curl -LsSf https://astral.sh/uv/install.sh | sh".to_string()))?;

        let envs_dir = dirs::home_dir()
            .map(|h| h.join(".ozzy/envs"))
            .unwrap_or_else(|| PathBuf::from(".ozzy/envs"));

        fs::create_dir_all(&envs_dir)?;

        Ok(Self {
            uv_path,
            envs_dir,
            platform: PlatformFingerprint::detect(),
        })
    }

    /// Get or create a virtual environment for a lockfile.
    pub fn get_env(&self, lockfile_hash: &str, python_version: &str) -> Result<PathBuf> {
        let env_name = format!("python-{}-{}", python_version, &lockfile_hash[..12]);
        let env_path = self.envs_dir.join(&env_name);

        if !env_path.exists() {
            // Environment doesn't exist, need to create it
            // For now, return None to indicate it needs creation
        }

        Ok(env_path)
    }

    /// Create a virtual environment from a lockfile.
    pub fn create_env(
        &self,
        lockfile_path: &Path,
        python_version: &str,
    ) -> Result<PathBuf> {
        let lockfile_hash = crate::hash::blake3_hash_file(lockfile_path)?;
        let env_name = format!("python-{}-{}", python_version, &lockfile_hash[..12]);
        let env_path = self.envs_dir.join(&env_name);

        if env_path.exists() {
            return Ok(env_path);
        }

        // Create virtual environment
        let status = Command::new(&self.uv_path)
            .args(["venv", "--python", python_version])
            .arg(&env_path)
            .status()
            .map_err(|e| Error::PythonError(format!("Failed to create venv: {}", e)))?;

        if !status.success() {
            return Err(Error::PythonError(format!(
                "uv venv failed with exit code: {:?}",
                status.code()
            )));
        }

        // Install dependencies from lockfile
        let status = Command::new(&self.uv_path)
            .args(["pip", "sync", "--python"])
            .arg(env_path.join("bin/python"))
            .arg(lockfile_path)
            .status()
            .map_err(|e| Error::PythonError(format!("Failed to sync dependencies: {}", e)))?;

        if !status.success() {
            return Err(Error::PythonError(format!(
                "uv pip sync failed with exit code: {:?}",
                status.code()
            )));
        }

        Ok(env_path)
    }

    /// Execute a Python transform.
    pub fn execute_transform(
        &self,
        env_path: &Path,
        transform_source: &Path,
        function_name: &str,
        input_path: &Path,
        output_path: &Path,
        params: &serde_json::Value,
    ) -> Result<()> {
        let python = env_path.join("bin/python");

        // Build the execution script
        let script = format!(
            r#"
import sys
import json
import polars as pl

# Load the transform module
sys.path.insert(0, "{transform_dir}")
import {module_name}

# Load input data
input_df = pl.read_parquet("{input_path}")

# Load params
params = json.loads('{params_json}')

# Execute transform
result = {module_name}.{function_name}(input_df, params)

# Write output
result.write_parquet("{output_path}")
"#,
            transform_dir = transform_source.parent().unwrap().display(),
            module_name = transform_source
                .file_stem()
                .unwrap()
                .to_string_lossy(),
            input_path = input_path.display(),
            params_json = serde_json::to_string(params)?.replace('\'', "\\'"),
            function_name = function_name,
            output_path = output_path.display(),
        );

        // Set up deterministic environment
        let mut env_vars: HashMap<String, String> = std::env::vars().collect();
        for (key, value) in DETERMINISTIC_ENV {
            env_vars.insert(key.to_string(), value.to_string());
        }

        let output = Command::new(&python)
            .args(["-c", &script])
            .envs(env_vars)
            .output()
            .map_err(|e| Error::PythonError(format!("Failed to execute Python: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::PythonError(format!(
                "Transform execution failed:\n{}",
                stderr
            )));
        }

        Ok(())
    }

    /// Get the platform fingerprint.
    pub fn platform(&self) -> &PlatformFingerprint {
        &self.platform
    }
}

impl Default for PythonRuntime {
    fn default() -> Self {
        Self::new().expect("Failed to initialize Python runtime")
    }
}

/// Execute a transform in the simplest way (for local development).
///
/// This doesn't use isolated environments - just runs Python directly.
/// Useful for quick iteration during development.
pub fn execute_transform_simple(
    transform_source: &Path,
    function_name: &str,
    input_path: &Path,
    output_path: &Path,
    params: &serde_json::Value,
) -> Result<()> {
    let python = which::which("python3")
        .or_else(|_| which::which("python"))
        .map_err(|_| Error::RuntimeError("Python not found in PATH".to_string()))?;

    // Build the execution script
    let script = format!(
        r#"
import sys
import json

# Try polars first, fall back to pandas
try:
    import polars as pl
    USE_POLARS = True
except ImportError:
    import pandas as pd
    USE_POLARS = False

# Load the transform module
sys.path.insert(0, "{transform_dir}")
import {module_name}

# Load input data
if USE_POLARS:
    input_df = pl.read_parquet("{input_path}")
else:
    input_df = pd.read_parquet("{input_path}")

# Load params
params = json.loads('{params_json}')

# Create a simple params object
class Params:
    def __init__(self, d):
        for k, v in d.items():
            setattr(self, k, v)

params_obj = Params(params)

# Execute transform
result = {module_name}.{function_name}({{"main": input_df}}, params_obj)

# Write output
if USE_POLARS:
    result.write_parquet("{output_path}")
else:
    result.to_parquet("{output_path}")
"#,
        transform_dir = transform_source.parent().unwrap().display(),
        module_name = transform_source.file_stem().unwrap().to_string_lossy(),
        input_path = input_path.display(),
        params_json = serde_json::to_string(params)?.replace('\'', "\\'"),
        function_name = function_name,
        output_path = output_path.display(),
    );

    // Set up deterministic environment
    let mut env_vars: HashMap<String, String> = std::env::vars().collect();
    for (key, value) in DETERMINISTIC_ENV {
        env_vars.insert(key.to_string(), value.to_string());
    }

    let output = Command::new(&python)
        .args(["-c", &script])
        .envs(env_vars)
        .output()
        .map_err(|e| Error::PythonError(format!("Failed to execute Python: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(Error::PythonError(format!(
            "Transform execution failed:\nstdout: {}\nstderr: {}",
            stdout, stderr
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deterministic_env_vars() {
        assert!(DETERMINISTIC_ENV.iter().any(|(k, _)| *k == "PYTHONHASHSEED"));
        assert!(DETERMINISTIC_ENV.iter().any(|(k, _)| *k == "OMP_NUM_THREADS"));
    }
}
