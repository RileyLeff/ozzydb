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
}
