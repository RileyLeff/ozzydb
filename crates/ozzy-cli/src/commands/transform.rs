//! `ozzy transform scaffold` — generate transform boilerplate.

use std::path::Path;

use anyhow::{Context, Result, bail};

/// Run the transform scaffold command.
pub fn scaffold(cwd: &Path, name: &str, lang: &str) -> Result<()> {
    // Validate name
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        bail!("Transform name must match [a-zA-Z0-9_-]+, got: '{}'", name);
    }

    match lang {
        "python" => scaffold_python(cwd, name),
        "r" => scaffold_r(cwd, name),
        _ => bail!("Unsupported language: '{}'. Supported: python, r", lang),
    }
}

fn scaffold_python(cwd: &Path, name: &str) -> Result<()> {
    let dir = cwd.join("transforms");
    std::fs::create_dir_all(&dir).context("Failed to create transforms/")?;

    let file_path = dir.join(format!("{}.py", name));
    if file_path.exists() {
        bail!("File already exists: transforms/{}.py", name);
    }

    let content = format!(
        r#"def {name}(inputs, params):
    """TODO: Implement transform logic.

    Args:
        inputs: dict of input data (keys match ozzy.toml input declarations)
        params: dict of parameters (keys match ozzy.toml param declarations)

    Returns:
        Output data (DataFrame, bytes, list for collections, etc.)
    """
    raise NotImplementedError("Implement this transform")
"#,
        name = name
    );

    std::fs::write(&file_path, content).context("Failed to write transform file")?;

    println!("Created transforms/{}.py", name);
    println!();
    println!("Add this to your ozzy.toml:");
    println!();
    println!("[transforms.{}]", name);
    println!("source = \"transforms/{}.py:{}\"", name, name);
    println!("environment = \"default\"");
    println!("inputs.data = \"parquet\"");
    println!("output = \"parquet\"");
    println!("# params.threshold = {{ type = \"float\" }}");

    Ok(())
}

fn scaffold_r(cwd: &Path, name: &str) -> Result<()> {
    let dir = cwd.join("transforms");
    std::fs::create_dir_all(&dir).context("Failed to create transforms/")?;

    let file_path = dir.join(format!("{}.R", name));
    if file_path.exists() {
        bail!("File already exists: transforms/{}.R", name);
    }

    let content = format!(
        r#"#' TODO: Implement transform logic.
#'
#' @param inputs Named list of input data (keys match ozzy.toml input declarations)
#' @param params Named list of parameters (keys match ozzy.toml param declarations)
#' @return Output data (data.frame, raw bytes, list for collections, etc.)
{name} <- function(inputs, params) {{
  stop("Implement this transform")
}}
"#,
        name = name
    );

    std::fs::write(&file_path, content).context("Failed to write transform file")?;

    println!("Created transforms/{}.R", name);
    println!();
    println!("Add this to your ozzy.toml:");
    println!();
    println!("[transforms.{}]", name);
    println!("source = \"transforms/{}.R:{}\"", name, name);
    println!("environment = \"default\"");
    println!("inputs.data = \"parquet\"");
    println!("output = \"parquet\"");
    println!("# params.threshold = {{ type = \"float\" }}");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scaffold_python() {
        let dir = tempfile::tempdir().unwrap();
        scaffold(dir.path(), "quality_control", "python").unwrap();

        let path = dir.path().join("transforms/quality_control.py");
        assert!(path.exists());

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("def quality_control(inputs, params):"));
        assert!(content.contains("NotImplementedError"));
    }

    #[test]
    fn test_scaffold_r() {
        let dir = tempfile::tempdir().unwrap();
        scaffold(dir.path(), "spatial_join", "r").unwrap();

        let path = dir.path().join("transforms/spatial_join.R");
        assert!(path.exists());

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("spatial_join <- function(inputs, params)"));
        assert!(content.contains("stop("));
    }

    #[test]
    fn test_scaffold_rejects_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        scaffold(dir.path(), "my_transform", "python").unwrap();

        // Second scaffold should fail
        let result = scaffold(dir.path(), "my_transform", "python");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }

    #[test]
    fn test_scaffold_rejects_invalid_name() {
        let dir = tempfile::tempdir().unwrap();
        let result = scaffold(dir.path(), "bad name!", "python");
        assert!(result.is_err());
    }

    #[test]
    fn test_scaffold_rejects_unsupported_lang() {
        let dir = tempfile::tempdir().unwrap();
        let result = scaffold(dir.path(), "test", "javascript");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unsupported"));
    }
}
