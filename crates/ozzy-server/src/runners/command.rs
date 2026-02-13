//! Command runner — shell template substitution for command-based transforms.
//!
//! Substitutes system-controlled template variables in the command string:
//! - `${input.NAME}` → `/workspace/inputs/NAME`
//! - `${output}` → `/workspace/output/result`
//!
//! **Security:** Parameters are NOT template-substituted. They're only accessible
//! via env vars (`$OZZY_PARAM_*`) and the params file (`/workspace/params.json`).
//! This prevents shell injection from consumer-provided param values.

use std::collections::HashMap;

/// Generate the substituted command string for a command-based transform.
///
/// `input_names` is the list of declared input names for this transform.
/// Only `${input.NAME}` and `${output}` are substituted.
pub fn generate(command: &str, input_names: &[&str]) -> String {
    let mut result = command.to_string();

    // Substitute ${input.NAME} → /workspace/inputs/NAME
    for name in input_names {
        let pattern = format!("${{input.{}}}", name);
        let replacement = format!("/workspace/inputs/{}", name);
        result = result.replace(&pattern, &replacement);
    }

    // Substitute ${output} → /workspace/output/result
    result = result.replace("${output}", "/workspace/output/result");

    result
}

/// Generate the full shell command that wraps the user's command.
///
/// Sets up the output directory and runs the substituted command via /bin/sh -c.
pub fn generate_shell_wrapper(command: &str, input_names: &[&str]) -> String {
    let substituted = generate(command, input_names);
    format!(
        "#!/bin/sh\nset -e\nmkdir -p /workspace/output\n{}\n",
        substituted
    )
}

/// Validate a command template for safety.
///
/// Returns a list of issues found (empty if clean).
pub fn validate_command(command: &str, declared_inputs: &HashMap<String, String>) -> Vec<String> {
    let mut issues = Vec::new();

    // Check for parameter template variables (not allowed — use env vars instead)
    if command.contains("${param.") || command.contains("${params.") {
        issues.push(
            "Command must not use ${param.*} templates. Use $OZZY_PARAM_* env vars instead."
                .to_string(),
        );
    }

    // Check that all ${input.NAME} references match declared inputs
    let mut pos = 0;
    while let Some(start) = command[pos..].find("${input.") {
        let abs_start = pos + start;
        if let Some(end) = command[abs_start..].find('}') {
            let ref_name = &command[abs_start + 8..abs_start + end];
            if !declared_inputs.contains_key(ref_name) {
                issues.push(format!(
                    "Command references undeclared input '{}'. Declared inputs: {:?}",
                    ref_name,
                    declared_inputs.keys().collect::<Vec<_>>()
                ));
            }
            pos = abs_start + end + 1;
        } else {
            issues.push("Unclosed ${input. template variable".to_string());
            break;
        }
    }

    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_basic() {
        let cmd = generate("ffmpeg -i ${input.video} -o ${output}", &["video"]);
        assert_eq!(
            cmd,
            "ffmpeg -i /workspace/inputs/video -o /workspace/output/result"
        );
    }

    #[test]
    fn test_generate_multiple_inputs() {
        let cmd = generate(
            "cat ${input.left} ${input.right} > ${output}",
            &["left", "right"],
        );
        assert_eq!(
            cmd,
            "cat /workspace/inputs/left /workspace/inputs/right > /workspace/output/result"
        );
    }

    #[test]
    fn test_generate_no_substitutions() {
        let cmd = generate("echo hello", &[]);
        assert_eq!(cmd, "echo hello");
    }

    #[test]
    fn test_generate_shell_wrapper() {
        let wrapper = generate_shell_wrapper("cat ${input.data} > ${output}", &["data"]);
        assert!(wrapper.starts_with("#!/bin/sh\n"));
        assert!(wrapper.contains("set -e"));
        assert!(wrapper.contains("mkdir -p /workspace/output"));
        assert!(wrapper.contains("/workspace/inputs/data"));
    }

    #[test]
    fn test_validate_command_clean() {
        let mut inputs = HashMap::new();
        inputs.insert("video".to_string(), "video/mp4".to_string());
        let issues = validate_command("ffmpeg -i ${input.video} ${output}", &inputs);
        assert!(issues.is_empty());
    }

    #[test]
    fn test_validate_command_rejects_param_templates() {
        let issues = validate_command("cmd --threshold=${param.threshold}", &HashMap::new());
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("${param.*}"));
    }

    #[test]
    fn test_validate_command_undeclared_input() {
        let inputs = HashMap::new();
        let issues = validate_command("cat ${input.missing}", &inputs);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("undeclared input 'missing'"));
    }

    #[test]
    fn test_validate_command_unclosed_template() {
        let issues = validate_command("cat ${input.broken", &HashMap::new());
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("Unclosed"));
    }
}
