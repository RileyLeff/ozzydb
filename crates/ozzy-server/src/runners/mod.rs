//! Runner generation — bridge scripts between OzzyDB's I/O contract and user code.
//!
//! Runners are small scripts injected into the compute container at runtime.
//! They handle:
//! 1. Loading inputs from the workspace via the input manifest
//! 2. Importing and calling the user's function
//! 3. Writing the output to the workspace
//!
//! Three types:
//! - **Python**: For `source = "path/to/file.py:function_name"`
//! - **R**: For `source = "path/to/file.R:function_name"`
//! - **Command**: For `command = "ffmpeg -i ${input.video} ..."` (shell template substitution)

pub mod command;
pub mod init;
pub mod python;
pub mod r;

/// Parse a transform source reference into (file_path, function_name).
///
/// Format: `"path/to/file.py:function_name"` → `("path/to/file.py", "function_name")`
pub fn parse_source_ref(source: &str) -> Option<(&str, &str)> {
    source.rsplit_once(':')
}

/// Validate a source reference for safety.
///
/// Rejects newlines (which could inject code into generated runner scripts)
/// and ensures the function name is a valid identifier.
pub fn validate_source_ref(source: &str) -> Result<(&str, &str), &'static str> {
    if source.contains('\n') || source.contains('\r') {
        return Err("source must not contain newlines");
    }
    let (file_path, func_name) =
        parse_source_ref(source).ok_or("source must contain ':' separator")?;
    if file_path.is_empty() {
        return Err("source file path must not be empty");
    }
    // Validate file_path characters to prevent code injection in generated runner scripts.
    // Python runner does `from {module} import ...` and R runner does `source("{file_path}")`,
    // so we restrict to safe path characters only.
    if !file_path
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '/' | '.' | '_' | '-'))
    {
        return Err("source file path contains invalid characters (allowed: alphanumeric, /, ., _, -)");
    }
    if func_name.is_empty() || !func_name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err("function name must be a valid identifier (alphanumeric + underscore)");
    }
    Ok((file_path, func_name))
}

/// Detect the runner type from the source file extension.
///
/// Returns `None` for unrecognized extensions so callers can surface a clear error.
pub fn detect_runner_type(source: &str) -> Option<RunnerType> {
    let (file_path, _) = parse_source_ref(source).unwrap_or((source, ""));
    if file_path.ends_with(".py") {
        Some(RunnerType::Python)
    } else if file_path.ends_with(".R") || file_path.ends_with(".r") {
        Some(RunnerType::R)
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunnerType {
    Python,
    R,
    Command,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_source_ref() {
        assert_eq!(
            parse_source_ref("transforms/qc.py:quality_control"),
            Some(("transforms/qc.py", "quality_control"))
        );
        assert_eq!(
            parse_source_ref("src/analysis.R:run_analysis"),
            Some(("src/analysis.R", "run_analysis"))
        );
        assert_eq!(parse_source_ref("no_colon.py"), None);
    }

    #[test]
    fn test_detect_runner_type() {
        assert_eq!(
            detect_runner_type("transforms/qc.py:func"),
            Some(RunnerType::Python)
        );
        assert_eq!(
            detect_runner_type("src/analysis.R:func"),
            Some(RunnerType::R)
        );
        assert_eq!(
            detect_runner_type("src/analysis.r:func"),
            Some(RunnerType::R)
        );
        assert_eq!(
            detect_runner_type("unknown.jl:func"),
            None // unsupported extension
        );
    }

    #[test]
    fn test_validate_source_ref_valid() {
        assert_eq!(
            validate_source_ref("transforms/qc.py:quality_control"),
            Ok(("transforms/qc.py", "quality_control"))
        );
        assert_eq!(
            validate_source_ref("src/analysis.R:run_analysis"),
            Ok(("src/analysis.R", "run_analysis"))
        );
    }

    #[test]
    fn test_validate_source_ref_rejects_newlines() {
        assert!(validate_source_ref("main.py:func\nimport os").is_err());
        assert!(validate_source_ref("main.py:func\rimport os").is_err());
    }

    #[test]
    fn test_validate_source_ref_rejects_invalid_func_name() {
        assert!(validate_source_ref("main.py:func name").is_err());
        assert!(validate_source_ref("main.py:func;evil").is_err());
        assert!(validate_source_ref("main.py:").is_err());
    }

    #[test]
    fn test_validate_source_ref_rejects_missing_colon() {
        assert!(validate_source_ref("main.py").is_err());
    }

    #[test]
    fn test_validate_source_ref_rejects_injection_in_file_path() {
        // Python injection: `from evil import os; os.system("bad"); # import func`
        assert!(validate_source_ref("evil import os; os.system(\"bad\"); #.py:func").is_err());
        // R injection: `source("/workspace/source/foo.R"); system("evil")`
        assert!(validate_source_ref("foo.R\"); system(\"evil:func").is_err());
        // Spaces in path
        assert!(validate_source_ref("bad path.py:func").is_err());
        // Parens in path
        assert!(validate_source_ref("evil().py:func").is_err());
        // Semicolons in path
        assert!(validate_source_ref("evil;cmd.py:func").is_err());
    }

    #[test]
    fn test_validate_source_ref_allows_valid_paths() {
        assert!(validate_source_ref("transforms/qc.py:func").is_ok());
        assert!(validate_source_ref("src/my-module/analysis_v2.py:run").is_ok());
        assert!(validate_source_ref("main.R:process").is_ok());
    }
}
