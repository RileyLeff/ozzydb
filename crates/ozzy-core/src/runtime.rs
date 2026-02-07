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

/// Escape a string for safe use in a Python string literal.
/// Handles backslashes, quotes, and newlines.
fn escape_python_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn build_sorted_input_load_code(inputs: &HashMap<String, PathBuf>) -> String {
    let mut sorted_inputs: Vec<_> = inputs.iter().collect();
    sorted_inputs.sort_by(|(name_a, _), (name_b, _)| name_a.cmp(name_b));

    sorted_inputs
        .into_iter()
        .map(|(name, path)| {
            format!(
                "inputs[\"{}\"] = pl.read_parquet(\"{}\")",
                escape_python_string(name),
                escape_python_string(&path.display().to_string())
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Deterministic environment variables for transform execution.
pub const DETERMINISTIC_ENV: &[(&str, &str)] = &[
    ("PYTHONHASHSEED", "0"),
    ("OMP_NUM_THREADS", "1"),
    ("MKL_NUM_THREADS", "1"),
    ("OPENBLAS_NUM_THREADS", "1"),
    ("NUMEXPR_NUM_THREADS", "1"),
];

/// Validate that a string is a valid Python identifier (safe for use in import/call statements).
fn validate_python_identifier(name: &str, context: &str) -> Result<()> {
    if name.is_empty() {
        return Err(Error::RuntimeError(format!("{} is empty", context)));
    }
    let first = name.chars().next().unwrap();
    if !first.is_ascii_alphabetic() && first != '_' {
        return Err(Error::RuntimeError(format!(
            "{} '{}' is not a valid Python identifier (must start with letter or underscore)",
            context, name
        )));
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(Error::RuntimeError(format!(
            "{} '{}' is not a valid Python identifier (only letters, digits, and underscores allowed)",
            context, name
        )));
    }
    Ok(())
}

/// Extract transform directory and module name from a transform source path.
fn extract_transform_info(transform_source: &Path) -> Result<(String, String)> {
    let transform_dir = transform_source
        .parent()
        .ok_or_else(|| {
            Error::RuntimeError(format!(
                "Transform path has no parent directory: {}",
                transform_source.display()
            ))
        })?
        .display()
        .to_string();

    let module_name = transform_source
        .file_stem()
        .ok_or_else(|| {
            Error::RuntimeError(format!(
                "Transform path has no file stem: {}",
                transform_source.display()
            ))
        })?
        .to_string_lossy()
        .to_string();

    validate_python_identifier(&module_name, "Module name")?;

    Ok((transform_dir, module_name))
}

fn lockfile_requirements(lockfile_path: &Path) -> Result<Vec<String>> {
    let content = fs::read_to_string(lockfile_path)?;
    let lockfile: toml::Value = toml::from_str(&content).map_err(|e| {
        Error::PythonError(format!(
            "Failed to parse uv.lock at {}: {}",
            lockfile_path.display(),
            e
        ))
    })?;

    let mut requirements = std::collections::BTreeSet::new();
    if let Some(packages) = lockfile.get("package").and_then(|v| v.as_array()) {
        for package in packages {
            let Some(pkg_table) = package.as_table() else {
                continue;
            };
            let Some(name) = pkg_table.get("name").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(version) = pkg_table.get("version").and_then(|v| v.as_str()) else {
                continue;
            };
            requirements.insert(format!("{}=={}", name, version));
        }
    }

    Ok(requirements.into_iter().collect())
}

fn normalized_python_version(raw: Option<&str>) -> String {
    match raw {
        Some(version) => {
            let mut parts = version.split('.');
            let major = parts.next().unwrap_or("3");
            let minor = parts.next().unwrap_or("11");
            format!("{}.{}", major, minor)
        }
        None => "3.11".to_string(),
    }
}

fn ensure_env_from_lockfile(
    uv_path: &Path,
    lockfile_path: &Path,
    envs_dir: &Path,
    python_version: &str,
) -> Result<PathBuf> {
    let lockfile_hash = blake3_hash_file(lockfile_path)?;
    let env_name = format!(
        "py{}-{}",
        python_version.replace('.', ""),
        &lockfile_hash[..12]
    );
    let env_path = envs_dir.join(env_name);
    let python_path = env_path.join("bin/python");

    if python_path.exists() {
        return Ok(env_path);
    }

    fs::create_dir_all(envs_dir)?;

    let venv_output = Command::new(uv_path)
        .args(["venv", "--python", python_version])
        .arg(&env_path)
        .output()
        .map_err(|e| Error::PythonError(format!("Failed to run uv venv: {}", e)))?;
    if !venv_output.status.success() {
        return Err(Error::PythonError(format!(
            "uv venv failed:\n{}",
            String::from_utf8_lossy(&venv_output.stderr)
        )));
    }

    let requirements = lockfile_requirements(lockfile_path)?;
    if requirements.is_empty() {
        return Ok(env_path);
    }

    let requirements_file = tempfile::NamedTempFile::new().map_err(|e| {
        Error::PythonError(format!("Failed to create temp requirements file: {}", e))
    })?;
    fs::write(requirements_file.path(), requirements.join("\n")).map_err(|e| {
        Error::PythonError(format!("Failed to write temp requirements file: {}", e))
    })?;

    let install_output = Command::new(uv_path)
        .args(["pip", "install", "--python"])
        .arg(&python_path)
        .args(["-r"])
        .arg(requirements_file.path())
        .output()
        .map_err(|e| Error::PythonError(format!("Failed to run uv pip install: {}", e)))?;
    if !install_output.status.success() {
        return Err(Error::PythonError(format!(
            "uv pip install failed:\n{}",
            String::from_utf8_lossy(&install_output.stderr)
        )));
    }

    Ok(env_path)
}

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
        let env_name = format!(
            "py{}-{}",
            python_version.replace('.', ""),
            lockfile_hash.get(..12).unwrap_or(lockfile_hash)
        );
        self.envs_dir.join(env_name)
    }

    /// Check if an environment exists.
    pub fn env_exists(&self, lockfile_hash: &str, python_version: &str) -> bool {
        let env_path = self.env_path(lockfile_hash, python_version);
        env_path.join("bin/python").exists()
    }

    /// Create a virtual environment from a requirements file or uv.lock.
    pub fn create_env(&self, requirements_path: &Path, python_version: &str) -> Result<PathBuf> {
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
            return Err(Error::PythonError(format!("uv venv failed: {}", stderr)));
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

        // Extract and validate transform info
        let (transform_dir, module_name) = extract_transform_info(transform_source)?;
        validate_python_identifier(function_name, "Function name")?;

        // Build input loading code with proper escaping
        let input_code = build_sorted_input_load_code(inputs);

        // Escape paths and identifiers for safe script generation
        let escaped_transform_dir = escape_python_string(&transform_dir);
        let escaped_output_path = escape_python_string(&output_path.display().to_string());
        let params_json = serde_json::to_string(params)?;
        let escaped_params = escape_python_string(&params_json);

        // Build the execution script
        let script = format!(
            r#"
import sys
import json
import polars as pl

# Load the transform module
sys.path.insert(0, "{escaped_transform_dir}")
import {module_name}

# Load inputs
inputs = {{}}
{input_code}

# Load params
params_dict = json.loads("{escaped_params}")

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
result.write_parquet("{escaped_output_path}")

print("SUCCESS")
"#,
            escaped_transform_dir = escaped_transform_dir,
            module_name = module_name,
            input_code = input_code,
            escaped_params = escaped_params,
            function_name = function_name,
            escaped_output_path = escaped_output_path,
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
        // Extract and validate transform info
        let (transform_dir, module_name) = extract_transform_info(transform_source)?;
        validate_python_identifier(function_name, "Function name")?;

        // Build input loading code with proper escaping
        let input_code = build_sorted_input_load_code(inputs);

        // Escape paths and identifiers for safe script generation
        let escaped_transform_dir = escape_python_string(&transform_dir);
        let escaped_output_path = escape_python_string(&output_path.display().to_string());
        let params_json = serde_json::to_string(params)?;
        let escaped_params = escape_python_string(&params_json);

        // Build the execution script
        let script = format!(
            r#"
import sys
import json
import polars as pl

sys.path.insert(0, "{escaped_transform_dir}")
import {module_name}

inputs = {{}}
{input_code}

params_dict = json.loads("{escaped_params}")

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

result.write_parquet("{escaped_output_path}")
print("SUCCESS")
"#,
            escaped_transform_dir = escaped_transform_dir,
            module_name = module_name,
            input_code = input_code,
            escaped_params = escaped_params,
            function_name = function_name,
            escaped_output_path = escaped_output_path,
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

    let (transform_dir, module_name) = extract_transform_info(transform_source)?;
    validate_python_identifier(function_name, "Function name")?;

    // Escape paths and identifiers for safe script generation
    let escaped_transform_dir = escape_python_string(&transform_dir);
    let escaped_input_path = escape_python_string(&input_path.display().to_string());
    let escaped_output_path = escape_python_string(&output_path.display().to_string());
    let params_json = serde_json::to_string(params)?;
    let escaped_params = escape_python_string(&params_json);

    let script = format!(
        r#"
import sys
import json
import polars as pl

sys.path.insert(0, "{escaped_transform_dir}")
import {module_name}

inputs = {{"main": pl.read_parquet("{escaped_input_path}")}}

params_dict = json.loads("{escaped_params}")

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

result.write_parquet("{escaped_output_path}")
"#,
        escaped_transform_dir = escaped_transform_dir,
        module_name = module_name,
        escaped_input_path = escaped_input_path,
        escaped_params = escaped_params,
        function_name = function_name,
        escaped_output_path = escaped_output_path,
    );

    // Set up deterministic environment
    let mut env_vars: HashMap<String, String> = std::env::vars().collect();
    for (key, value) in DETERMINISTIC_ENV {
        env_vars.insert(key.to_string(), value.to_string());
    }

    let output = Command::new(&uv_path)
        .args([
            "run", "--with", "polars", "--with", "pyarrow", "python", "-c", &script,
        ])
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
        return execute_transform_uv(
            transform_source,
            function_name,
            input_path,
            output_path,
            params,
        );
    }

    let python = which::which("python3")
        .or_else(|_| which::which("python"))
        .map_err(|_| Error::RuntimeError("Neither uv nor python found in PATH".to_string()))?;

    let (transform_dir, module_name) = extract_transform_info(transform_source)?;
    validate_python_identifier(function_name, "Function name")?;

    // Escape paths and identifiers for safe script generation
    let escaped_transform_dir = escape_python_string(&transform_dir);
    let escaped_input_path = escape_python_string(&input_path.display().to_string());
    let escaped_output_path = escape_python_string(&output_path.display().to_string());
    let params_json = serde_json::to_string(params)?;
    let escaped_params = escape_python_string(&params_json);

    let script = format!(
        r#"
import sys
import json
import polars as pl

sys.path.insert(0, "{escaped_transform_dir}")
import {module_name}

inputs = {{"main": pl.read_parquet("{escaped_input_path}")}}

params_dict = json.loads("{escaped_params}")

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

result.write_parquet("{escaped_output_path}")
"#,
        escaped_transform_dir = escaped_transform_dir,
        module_name = module_name,
        escaped_input_path = escaped_input_path,
        escaped_params = escaped_params,
        function_name = function_name,
        escaped_output_path = escaped_output_path,
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

/// Execute a transform with multiple named inputs using uv run.
/// This is the primary execution method for multi-input transforms.
pub fn execute_transform_multi(
    transform_source: &Path,
    function_name: &str,
    inputs: &HashMap<String, PathBuf>,
    output_path: &Path,
    params: &serde_json::Value,
) -> Result<()> {
    let uv_path = which::which("uv").map_err(|_| {
        Error::RuntimeError(
            "uv not found in PATH. Install with: curl -LsSf https://astral.sh/uv/install.sh | sh"
                .to_string(),
        )
    })?;

    let (transform_dir, module_name) = extract_transform_info(transform_source)?;
    validate_python_identifier(function_name, "Function name")?;

    // Build input loading code for all inputs with proper escaping
    let input_code = build_sorted_input_load_code(inputs);

    // Escape paths and identifiers for safe script generation
    let escaped_transform_dir = escape_python_string(&transform_dir);
    let escaped_output_path = escape_python_string(&output_path.display().to_string());
    let params_json = serde_json::to_string(params)?;
    let escaped_params = escape_python_string(&params_json);

    let script = format!(
        r#"
import sys
import json
import polars as pl

sys.path.insert(0, "{escaped_transform_dir}")
import {module_name}

inputs = {{}}
{input_code}

params_dict = json.loads("{escaped_params}")

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

result.write_parquet("{escaped_output_path}")
"#,
        escaped_transform_dir = escaped_transform_dir,
        module_name = module_name,
        input_code = input_code,
        escaped_params = escaped_params,
        function_name = function_name,
        escaped_output_path = escaped_output_path,
    );

    // Set up deterministic environment
    let mut env_vars: HashMap<String, String> = std::env::vars().collect();
    for (key, value) in DETERMINISTIC_ENV {
        env_vars.insert(key.to_string(), value.to_string());
    }

    let lockfile_path = transform_source
        .parent()
        .map(|p| p.join("uv.lock"))
        .unwrap_or_else(|| PathBuf::from("uv.lock"));

    let output = if lockfile_path.exists() {
        let envs_dir = dirs::home_dir()
            .map(|h| h.join(".ozzy/envs"))
            .unwrap_or_else(|| PathBuf::from(".ozzy/envs"));
        let python_version =
            normalized_python_version(PlatformFingerprint::detect().python_version.as_deref());
        let env_path =
            ensure_env_from_lockfile(&uv_path, &lockfile_path, &envs_dir, &python_version)?;
        let python_path = env_path.join("bin/python");
        Command::new(&python_path)
            .args(["-c", &script])
            .envs(env_vars)
            .output()
            .map_err(|e| {
                Error::PythonError(format!(
                    "Failed to execute lockfile environment Python: {}",
                    e
                ))
            })?
    } else {
        Command::new(&uv_path)
            .args([
                "run", "--with", "polars", "--with", "pyarrow", "python", "-c", &script,
            ])
            .envs(env_vars)
            .output()
            .map_err(|e| Error::PythonError(format!("Failed to execute uv run: {}", e)))?
    };

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
        assert!(
            DETERMINISTIC_ENV
                .iter()
                .any(|(k, _)| *k == "PYTHONHASHSEED")
        );
        assert!(
            DETERMINISTIC_ENV
                .iter()
                .any(|(k, _)| *k == "OMP_NUM_THREADS")
        );
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

    #[test]
    fn test_build_sorted_input_load_code_orders_by_key() {
        let mut inputs = HashMap::new();
        inputs.insert("right".to_string(), PathBuf::from("/tmp/right.parquet"));
        inputs.insert("left".to_string(), PathBuf::from("/tmp/left.parquet"));

        let code = build_sorted_input_load_code(&inputs);
        let left_idx = code.find("inputs[\"left\"]").unwrap();
        let right_idx = code.find("inputs[\"right\"]").unwrap();

        assert!(left_idx < right_idx);
    }
}
