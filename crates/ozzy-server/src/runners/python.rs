//! Python runner generation.
//!
//! Generates a Python script that:
//! 1. Loads inputs via the OZZY_INPUT_MANIFEST env var
//! 2. Imports the user's function from the source file
//! 3. Calls it with (inputs, params)
//! 4. Writes the output to /workspace/output/

/// Generate a Python runner script for a transform.
///
/// The `module_path` is the Python import path relative to `/workspace/source/`,
/// and `function_name` is the function to call.
///
/// Example: `generate("transforms/qc.py", "quality_control")` produces a script
/// that does `from transforms.qc import quality_control`.
pub fn generate(source_file: &str, function_name: &str) -> String {
    // Convert file path to Python module path:
    // "transforms/qc.py" → "transforms.qc"
    let module = source_file
        .strip_suffix(".py")
        .unwrap_or(source_file)
        .replace('/', ".");

    format!(
        r#"#!/usr/bin/env python3
"""OzzyDB Python runner. Auto-generated — do not edit."""
import sys
import os
import json

sys.path.insert(0, '/workspace/source')

# --- Load params ---
params = json.loads(os.environ.get("OZZY_PARAMS", "{{}}"))

# --- Load inputs ---
input_manifest = json.loads(os.environ.get("OZZY_INPUT_MANIFEST", "{{}}"))


def _load_item(path, content_type):
    if "parquet" in content_type:
        import polars as pl
        return pl.read_parquet(path)
    elif content_type.startswith("image/"):
        with open(path, "rb") as f:
            return f.read()
    elif content_type == "application/json":
        with open(path) as f:
            return json.loads(f.read())
    elif content_type.startswith("text/"):
        with open(path) as f:
            return f.read()
    else:
        with open(path, "rb") as f:
            return f.read()


inputs = {{}}
for name, spec in input_manifest.items():
    path = spec["path"]
    content_type = spec["content_type"]
    is_collection = spec.get("is_collection", False)

    if is_collection:
        with open(spec["manifest_path"]) as f:
            member_manifest = json.loads(f.read())
        members = []
        for member in member_manifest:
            members.append(_load_item(member["path"], member["content_type"]))
        inputs[name] = members
    else:
        inputs[name] = _load_item(path, content_type)


def _write_item(item, path):
    if hasattr(item, 'collect'):
        item = item.collect()  # LazyFrame -> DataFrame
    if hasattr(item, 'write_parquet'):
        item.write_parquet(path + ".parquet")
    elif isinstance(item, (bytes, bytearray)):
        with open(path, "wb") as f:
            f.write(item)
    elif isinstance(item, str):
        with open(path, "w") as f:
            f.write(item)
    elif isinstance(item, dict):
        with open(path + ".json", "w") as f:
            json.dump(item, f)
    else:
        raise TypeError(f"Unsupported output type: {{type(item)}}")


# --- Import and call the user's function ---
from {module} import {function_name}

result = {function_name}(inputs, params)

# --- Handle output ---
output_dir = "/workspace/output"
os.makedirs(output_dir, exist_ok=True)

if isinstance(result, list):
    manifest = []
    for i, item in enumerate(result):
        out_path = os.path.join(output_dir, f"item_{{i:06d}}")
        _write_item(item, out_path)
        manifest.append({{"index": i, "path": out_path}})
    with open(os.path.join(output_dir, "manifest.json"), "w") as f:
        json.dump(manifest, f)
else:
    out_path = os.path.join(output_dir, "result")
    _write_item(result, out_path)
"#,
        module = module,
        function_name = function_name,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_basic() {
        let script = generate("transforms/qc.py", "quality_control");
        assert!(script.contains("from transforms.qc import quality_control"));
        assert!(script.contains("result = quality_control(inputs, params)"));
        assert!(script.contains("OZZY_INPUT_MANIFEST"));
        assert!(script.contains("OZZY_PARAMS"));
    }

    #[test]
    fn test_generate_nested_module() {
        let script = generate("src/analysis/pipeline.py", "run");
        assert!(script.contains("from src.analysis.pipeline import run"));
    }

    #[test]
    fn test_generate_top_level_module() {
        let script = generate("main.py", "process");
        assert!(script.contains("from main import process"));
    }

    #[test]
    fn test_generate_has_shebang() {
        let script = generate("f.py", "func");
        assert!(script.starts_with("#!/usr/bin/env python3"));
    }

    #[test]
    fn test_generate_handles_collection_output() {
        let script = generate("f.py", "func");
        assert!(script.contains("isinstance(result, list)"));
        assert!(script.contains("manifest.json"));
    }

    #[test]
    fn test_generate_handles_parquet_input() {
        let script = generate("f.py", "func");
        assert!(script.contains("polars"));
        assert!(script.contains("read_parquet"));
    }
}
