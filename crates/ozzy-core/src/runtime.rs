//! Runtime execution for Python transforms.
//!
//! Executes transforms in isolated uv-managed environments with deterministic settings.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{Error, Result};
use crate::hash::blake3_hash_file;
use crate::platform::PlatformFingerprint;

/// Deterministic environment variables for transform execution.
pub const DETERMINISTIC_ENV: &[(&str, &str)] = &[
    ("PYTHONHASHSEED", "0"),
    ("OMP_NUM_THREADS", "1"),
    ("MKL_NUM_THREADS", "1"),
    ("OPENBLAS_NUM_THREADS", "1"),
    ("NUMEXPR_NUM_THREADS", "1"),
];

/// Python runtime for executing transforms using uv.
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
        let uv_path = which::which("uv").map_err(|_| {
            Error::RuntimeError(
                "uv not found in PATH. Install with: curl -LsSf https://astral.sh/uv/install.sh | sh"
                    .to_string(),
            )
        })?;

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

    /// Get the environment directory for a given lockfile hash and python version.
    pub fn env_path(&self, lockfile_hash: &str, python_version: &str) -> PathBuf {
        let env_name = format!("py{}-{}", python_version.replace('.', ""), &lockfile_hash[..12]);
        self.envs_dir.join(env_name)
    }

    /// Check if an environment exists.
    pub fn env_exists(&self, lockfile_hash: &str, python_version: &str) -> bool {
        let env_path = self.env_path(lockfile_hash, python_version);
        env_path.join("bin/python").exists()
    }

    /// Create a virtual environment from a requirements file or uv.lock.
    pub fn create_env(
        &self,
        requirements_path: &Path,
        python_version: &str,
    ) -> Result<PathBuf> {
        let lockfile_hash = blake3_hash_file(requirements_path)?;
        let env_path = self.env_path(&lockfile_hash, python_version);

        if env_path.join("bin/python").exists() {
            return Ok(env_path);
        }

        // Create virtual environment
        let output = Command::new(&self.uv_path)
            .args(["venv", "--python", python_version])
            .arg(&env_path)
            .output()
            .map_err(|e| Error::PythonError(format!("Failed to run uv venv: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::PythonError(format!(
                "uv venv failed: {}",
                stderr
            )));
        }

        // Install dependencies
        let output = Command::new(&self.uv_path)
            .args(["pip", "install", "--python"])
            .arg(env_path.join("bin/python"))
            .args(["-r"])
            .arg(requirements_path)
            .output()
            .map_err(|e| Error::PythonError(format!("Failed to run uv pip install: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::PythonError(format!(
                "uv pip install failed: {}",
                stderr
            )));
        }

        Ok(env_path)
    }

    /// Execute a Python transform in an isolated environment.
    pub fn execute_transform(
        &self,
        env_path: &Path,
        transform_source: &Path,
        function_name: &str,
        inputs: &HashMap<String, PathBuf>,
        output_path: &Path,
        params: &serde_json::Value,
    ) -> Result<()> {
        let python = env_path.join("bin/python");

        if !python.exists() {
            return Err(Error::RuntimeError(format!(
                "Python not found in environment: {}",
                env_path.display()
            )));
        }

        // Build input loading code
        let input_code: String = inputs
            .iter()
            .map(|(name, path)| {
                format!(
                    "inputs[\"{}\"] = pl.read_parquet(\"{}\")",
                    name,
                    path.display()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        // Build the execution script
        let script = format!(
            r#"
import sys
import json
import polars as pl

# Load the transform module
sys.path.insert(0, "{transform_dir}")
import {module_name}

# Load inputs
inputs = {{}}
{input_code}

# Load params
params_dict = json.loads('{params_json}')

# Create params object with attribute access
class Params:
    def __init__(self, d):
        for k, v in d.items():
            setattr(self, k, v)
    def get(self, key, default=None):
        return getattr(self, key, default)

params = Params(params_dict)

# Execute transform
result = {module_name}.{function_name}(inputs, params)

# Handle LazyFrame
if hasattr(result, 'collect'):
    result = result.collect()

# Write output
result.write_parquet("{output_path}")

print("SUCCESS")
"#,
            transform_dir = transform_source.parent().unwrap().display(),
            module_name = transform_source.file_stem().unwrap().to_string_lossy(),
            input_code = input_code,
            params_json = serde_json::to_string(params)?.replace('\'', "\\'").replace('\n', "\\n"),
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

    /// Execute a transform using `uv run` with inline dependencies.
    /// This is useful when no lockfile exists - uv will manage deps automatically.
    pub fn execute_with_uv_run(
        &self,
        transform_source: &Path,
        function_name: &str,
        inputs: &HashMap<String, PathBuf>,
        output_path: &Path,
        params: &serde_json::Value,
        dependencies: &[&str],
    ) -> Result<()> {
        // Build input loading code
        let input_code: String = inputs
            .iter()
            .map(|(name, path)| {
                format!(
                    "inputs[\"{}\"] = pl.read_parquet(\"{}\")",
                    name,
                    path.display()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        // Build the execution script
        let script = format!(
            r#"
import sys
import json
import polars as pl

sys.path.insert(0, "{transform_dir}")
import {module_name}

inputs = {{}}
{input_code}

params_dict = json.loads('{params_json}')

class Params:
    def __init__(self, d):
        for k, v in d.items():
            setattr(self, k, v)
    def get(self, key, default=None):
        return getattr(self, key, default)

params = Params(params_dict)
result = {module_name}.{function_name}(inputs, params)

if hasattr(result, 'collect'):
    result = result.collect()

result.write_parquet("{output_path}")
print("SUCCESS")
"#,
            transform_dir = transform_source.parent().unwrap().display(),
            module_name = transform_source.file_stem().unwrap().to_string_lossy(),
            input_code = input_code,
            params_json = serde_json::to_string(params)?.replace('\'', "\\'").replace('\n', "\\n"),
            function_name = function_name,
            output_path = output_path.display(),
        );

        // Build uv run command with dependencies
        let mut cmd = Command::new(&self.uv_path);
        cmd.arg("run");

        // Add each dependency as --with
        for dep in dependencies {
            cmd.args(["--with", dep]);
        }

        cmd.args(["python", "-c", &script]);

        // Set up deterministic environment
        let mut env_vars: HashMap<String, String> = std::env::vars().collect();
        for (key, value) in DETERMINISTIC_ENV {
            env_vars.insert(key.to_string(), value.to_string());
        }
        cmd.envs(env_vars);

        let output = cmd
            .output()
            .map_err(|e| Error::PythonError(format!("Failed to execute uv run: {}", e)))?;

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

    /// Get the platform fingerprint.
    pub fn platform(&self) -> &PlatformFingerprint {
        &self.platform
    }

    /// Get the uv path.
    pub fn uv_path(&self) -> &Path {
        &self.uv_path
    }
}

impl Default for PythonRuntime {
    fn default() -> Self {
        Self::new().expect("Failed to initialize Python runtime")
    }
}

/// Execute a transform using uv run with polars (default simple execution).
/// This is the recommended way to run transforms - no manual env management needed.
pub fn execute_transform_uv(
    transform_source: &Path,
    function_name: &str,
    input_path: &Path,
    output_path: &Path,
    params: &serde_json::Value,
) -> Result<()> {
    let uv_path = which::which("uv").map_err(|_| {
        Error::RuntimeError(
            "uv not found in PATH. Install with: curl -LsSf https://astral.sh/uv/install.sh | sh"
                .to_string(),
        )
    })?;

    let script = format!(
        r#"
import sys
import json
import polars as pl

sys.path.insert(0, "{transform_dir}")
import {module_name}

inputs = {{"main": pl.read_parquet("{input_path}")}}

params_dict = json.loads('{params_json}')

class Params:
    def __init__(self, d):
        for k, v in d.items():
            setattr(self, k, v)
    def get(self, key, default=None):
        return getattr(self, key, default)

params = Params(params_dict)
result = {module_name}.{function_name}(inputs, params)

if hasattr(result, 'collect'):
    result = result.collect()

result.write_parquet("{output_path}")
"#,
        transform_dir = transform_source.parent().unwrap().display(),
        module_name = transform_source.file_stem().unwrap().to_string_lossy(),
        input_path = input_path.display(),
        params_json = serde_json::to_string(params)?.replace('\'', "\\'").replace('\n', "\\n"),
        function_name = function_name,
        output_path = output_path.display(),
    );

    // Set up deterministic environment
    let mut env_vars: HashMap<String, String> = std::env::vars().collect();
    for (key, value) in DETERMINISTIC_ENV {
        env_vars.insert(key.to_string(), value.to_string());
    }

    let output = Command::new(&uv_path)
        .args(["run", "--with", "polars", "--with", "pyarrow", "python", "-c", &script])
        .envs(env_vars)
        .output()
        .map_err(|e| Error::PythonError(format!("Failed to execute uv run: {}", e)))?;

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

/// Simple execution using system Python (for development/testing).
pub fn execute_transform_simple(
    transform_source: &Path,
    function_name: &str,
    input_path: &Path,
    output_path: &Path,
    params: &serde_json::Value,
) -> Result<()> {
    // Try uv first, fall back to system python
    if which::which("uv").is_ok() {
        return execute_transform_uv(transform_source, function_name, input_path, output_path, params);
    }

    let python = which::which("python3")
        .or_else(|_| which::which("python"))
        .map_err(|_| Error::RuntimeError("Neither uv nor python found in PATH".to_string()))?;

    let script = format!(
        r#"
import sys
import json
import polars as pl

sys.path.insert(0, "{transform_dir}")
import {module_name}

inputs = {{"main": pl.read_parquet("{input_path}")}}

params_dict = json.loads('{params_json}')

class Params:
    def __init__(self, d):
        for k, v in d.items():
            setattr(self, k, v)
    def get(self, key, default=None):
        return getattr(self, key, default)

params = Params(params_dict)
result = {module_name}.{function_name}(inputs, params)

if hasattr(result, 'collect'):
    result = result.collect()

result.write_parquet("{output_path}")
"#,
        transform_dir = transform_source.parent().unwrap().display(),
        module_name = transform_source.file_stem().unwrap().to_string_lossy(),
        input_path = input_path.display(),
        params_json = serde_json::to_string(params)?.replace('\'', "\\'").replace('\n', "\\n"),
        function_name = function_name,
        output_path = output_path.display(),
    );

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

    #[test]
    fn test_env_path_format() {
        let runtime = PythonRuntime {
            uv_path: PathBuf::from("/usr/bin/uv"),
            envs_dir: PathBuf::from("/tmp/envs"),
            platform: PlatformFingerprint::detect(),
        };

        let path = runtime.env_path("abcdef123456789", "3.11");
        assert!(path.to_string_lossy().contains("py311-abcdef123456"));
    }
}
