//! R runner generation.
//!
//! Generates an R script that:
//! 1. Loads inputs via the OZZY_INPUT_MANIFEST env var
//! 2. Sources the user's R file and calls the function
//! 3. Writes the output to /workspace/output/

/// Generate an R runner script for a transform.
pub fn generate(source_file: &str, function_name: &str) -> String {
    format!(
        r#"#!/usr/bin/env Rscript
# OzzyDB R runner. Auto-generated — do not edit.
library(jsonlite)
library(arrow)

# Load params
params <- fromJSON(Sys.getenv("OZZY_PARAMS", "{{}}"))

# Load input manifest
input_manifest <- fromJSON(Sys.getenv("OZZY_INPUT_MANIFEST", "{{}}"))
inputs <- list()

for (name in names(input_manifest)) {{
  spec <- input_manifest[[name]]
  if (grepl("parquet", spec$content_type)) {{
    inputs[[name]] <- read_parquet(spec$path)
  }} else if (spec$content_type == "text/csv") {{
    inputs[[name]] <- read.csv(spec$path)
  }} else if (startsWith(spec$content_type, "text/")) {{
    inputs[[name]] <- readLines(spec$path, warn = FALSE)
  }} else {{
    inputs[[name]] <- readBin(spec$path, "raw", file.info(spec$path)$size)
  }}
}}

# Source the user's file and call the function
source("/workspace/source/{source_file}")
result <- {function_name}(inputs, params)

# Write output
output_dir <- "/workspace/output"
dir.create(output_dir, showWarnings = FALSE, recursive = TRUE)

if (inherits(result, "data.frame") || inherits(result, "ArrowTabular")) {{
  write_parquet(result, file.path(output_dir, "result.parquet"))
}} else if (is.character(result)) {{
  writeLines(result, file.path(output_dir, "result.txt"))
}} else if (is.raw(result)) {{
  writeBin(result, file.path(output_dir, "result.bin"))
}} else {{
  saveRDS(result, file.path(output_dir, "result.rds"))
}}
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
        let script = generate("transforms/analysis.R", "run_analysis");
        assert!(script.contains("source(\"/workspace/source/transforms/analysis.R\")"));
        assert!(script.contains("result <- run_analysis(inputs, params)"));
        assert!(script.contains("OZZY_INPUT_MANIFEST"));
        assert!(script.contains("OZZY_PARAMS"));
    }

    #[test]
    fn test_generate_has_shebang() {
        let script = generate("f.R", "func");
        assert!(script.starts_with("#!/usr/bin/env Rscript"));
    }

    #[test]
    fn test_generate_uses_arrow() {
        let script = generate("f.R", "func");
        assert!(script.contains("library(arrow)"));
        assert!(script.contains("read_parquet"));
        assert!(script.contains("write_parquet"));
    }

    #[test]
    fn test_generate_handles_csv() {
        let script = generate("f.R", "func");
        assert!(script.contains("read.csv"));
    }
}
