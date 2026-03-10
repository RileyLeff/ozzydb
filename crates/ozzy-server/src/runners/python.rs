//! Python runner generation.
//!
//! Generates a Python script that:
//! 1. Loads inputs via the OZZY_INPUT_MANIFEST env var
//! 2. Imports the user's function from the source file
//! 3. Calls it with (inputs, params)
//! 4. Writes the output to /workspace/output/

/// Generate a Python runner script for a transform.
///
/// Uses `importlib` to load the module from a file path, which handles
/// any valid filename (including hyphens, dots, etc.) that would break
/// dotted `from X import Y` syntax.
pub fn generate(source_file: &str, function_name: &str) -> String {
    format!(
        r#"#!/usr/bin/env python3
"""OzzyDB Python runner. Auto-generated — do not edit."""
import sys
import os
import json
import importlib.util

sys.path.insert(0, '/workspace/source')

# --- Load params ---
params = json.loads(os.environ.get("OZZY_PARAMS", "{{}}"))

# --- Load inputs ---
input_manifest = json.loads(os.environ.get("OZZY_INPUT_MANIFEST", "{{}}"))

def _load_blob(spec):
    loader = spec["loader"]
    path = spec["path"]
    if loader == "parquet":
        import polars as pl
        return pl.read_parquet(path)
    elif loader == "csv":
        import polars as pl
        return pl.read_csv(path)
    elif loader == "json":
        with open(path) as f:
            return json.loads(f.read())
    elif loader == "text":
        with open(path) as f:
            return f.read()
    elif loader == "bytes":
        with open(path, "rb") as f:
            return f.read()
    else:
        raise ValueError(f"Unsupported input loader: {{loader}}")


def _load_input(spec):
    kind = spec["kind"]
    if kind == "blob":
        return _load_blob(spec)
    elif kind == "collection":
        return [_load_input(item) for item in spec["items"]]
    elif kind == "bundle":
        return {{name: _load_input(entry) for name, entry in spec["entries"].items()}}
    else:
        raise ValueError(f"Unsupported input kind: {{kind}}")


inputs = {{}}
for name, spec in input_manifest.items():
    inputs[name] = _load_input(spec)


def _write_item(item, path):
    """Write a single output item. Returns the actual path written."""
    if hasattr(item, 'collect'):
        item = item.collect()  # LazyFrame -> DataFrame
    if hasattr(item, 'write_parquet'):
        actual = path + ".parquet"
        item.write_parquet(actual)
        return actual
    elif isinstance(item, (bytes, bytearray)):
        with open(path, "wb") as f:
            f.write(item)
        return path
    elif isinstance(item, str):
        with open(path, "w") as f:
            f.write(item)
        return path
    elif isinstance(item, dict):
        actual = path + ".json"
        with open(actual, "w") as f:
            json.dump(item, f)
        return actual
    else:
        raise TypeError(f"Unsupported output type: {{type(item)}}")


# --- Import and call the user's function ---
_source_path = os.path.join('/workspace/source', '{source_file}')
_spec = importlib.util.spec_from_file_location('_ozzy_user_module', _source_path)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)
{function_name} = getattr(_mod, '{function_name}')

result = {function_name}(inputs, params)

# --- Handle output ---
output_dir = "/workspace/output"
os.makedirs(output_dir, exist_ok=True)

if isinstance(result, list):
    manifest = []
    for i, item in enumerate(result):
        out_path = os.path.join(output_dir, f"item_{{i:06d}}")
        actual_path = _write_item(item, out_path)
        manifest.append({{"index": i, "path": actual_path}})
    with open(os.path.join(output_dir, "manifest.json"), "w") as f:
        json.dump(manifest, f)
else:
    out_path = os.path.join(output_dir, "result")
    _write_item(result, out_path)
"#,
        source_file = source_file,
        function_name = function_name,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_basic() {
        let script = generate("transforms/qc.py", "quality_control");
        assert!(script.contains("importlib.util.spec_from_file_location"));
        assert!(script.contains("'transforms/qc.py'"));
        assert!(script.contains("quality_control = getattr(_mod, 'quality_control')"));
        assert!(script.contains("result = quality_control(inputs, params)"));
        assert!(script.contains("OZZY_INPUT_MANIFEST"));
        assert!(script.contains("OZZY_PARAMS"));
    }

    #[test]
    fn test_generate_nested_module() {
        let script = generate("src/analysis/pipeline.py", "run");
        assert!(script.contains("'src/analysis/pipeline.py'"));
        assert!(script.contains("run = getattr(_mod, 'run')"));
    }

    #[test]
    fn test_generate_top_level_module() {
        let script = generate("main.py", "process");
        assert!(script.contains("'main.py'"));
        assert!(script.contains("process = getattr(_mod, 'process')"));
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

    #[test]
    fn test_generate_handles_hyphenated_path() {
        // Hyphens in file paths are valid but would break dotted imports.
        // importlib approach handles this correctly.
        let script = generate("my-transforms/qc-check.py", "run_check");
        assert!(script.contains("'my-transforms/qc-check.py'"));
        assert!(script.contains("run_check = getattr(_mod, 'run_check')"));
    }
}
