//! Init script generation — the container entrypoint.
//!
//! The init script is the first thing that runs inside the compute container.
//! It uses presigned URLs for all I/O — downloading inputs, source code, secrets,
//! and uploading output. This is the same script for all compute backends
//! (Docker, Fly Machines, etc.).
//!
//! **Requires:** `python3` (for secrets + input downloads) and `curl` (for
//! source code download + output upload) in the environment image.

use super::RunnerType;

/// Generate the unified init script for compute containers.
///
/// All I/O happens via presigned URLs passed as env vars:
/// - `OZZY_RUNNER_SCRIPT_B64`: base64-encoded runner script
/// - `OZZY_INPUT_DOWNLOADS`: JSON array of `{name, url, path}` for inputs
/// - `OZZY_SOURCE_DOWNLOAD`: presigned GET URL for source code tarball
/// - `OZZY_OUTPUT_UPLOAD_URL`: presigned PUT URL for output tarball
/// - `OZZY_SECRETS_URL`: presigned GET URL for secrets JSON blob
pub fn generate_init(runner_type: RunnerType) -> String {
    let runner_ext = match runner_type {
        RunnerType::Python => "py",
        RunnerType::R => "R",
        RunnerType::Command => "sh",
    };

    let runner_cmd = match runner_type {
        RunnerType::Python => "python3 /workspace/runner.py",
        RunnerType::R => "Rscript /workspace/runner.R",
        RunnerType::Command => "/bin/sh /workspace/runner.sh",
    };

    format!(
        r#"#!/bin/sh
set -e

echo "OzzyDB init: starting"

# Download and export secrets (if OZZY_SECRETS_URL is set)
if [ -n "$OZZY_SECRETS_URL" ]; then
    python3 -c "
import json, os, sys, urllib.request
try:
    data = urllib.request.urlopen(os.environ['OZZY_SECRETS_URL']).read()
    secrets = json.loads(data)
    with open('/tmp/secrets.env', 'w') as f:
        for k, v in secrets.items():
            v_esc = v.replace(chr(39), chr(39)+chr(92)+chr(39)+chr(39))
            f.write('export ' + k + '=' + chr(39) + v_esc + chr(39) + chr(10))
    print('Loaded ' + str(len(secrets)) + ' secret(s)')
except Exception as e:
    print('ERROR: Failed to load secrets: ' + str(e), file=sys.stderr)
    sys.exit(1)
" || exit 1
    . /tmp/secrets.env
    rm -f /tmp/secrets.env
fi

# Decode runner script from env var
echo "$OZZY_RUNNER_SCRIPT_B64" | base64 -d > /workspace/runner.{runner_ext}
chmod +x /workspace/runner.{runner_ext}

# Download source code from R2 (if any — source-based transforms need /workspace/source/)
if [ -n "$OZZY_SOURCE_DOWNLOAD" ]; then
    mkdir -p /workspace/source
    curl -sS -o /tmp/source.tar.gz "$OZZY_SOURCE_DOWNLOAD" || {{
        echo "ERROR: Failed to download source code" >&2
        exit 1
    }}
    tar xzf /tmp/source.tar.gz -C /workspace/source
    rm -f /tmp/source.tar.gz
fi

# Save output upload URL for later, then unset infrastructure env vars
echo "$OZZY_OUTPUT_UPLOAD_URL" > /tmp/ozzy_upload_url
unset OZZY_RUNNER_SCRIPT_B64
unset OZZY_INIT_SCRIPT_B64
unset OZZY_SECRETS_URL
unset OZZY_OUTPUT_UPLOAD_URL
unset OZZY_SOURCE_DOWNLOAD

# Download inputs from presigned URLs (if any)
mkdir -p /workspace/inputs
if [ -n "$OZZY_INPUT_DOWNLOADS" ]; then
    DOWNLOADS=$(echo "$OZZY_INPUT_DOWNLOADS" | python3 -c "
import sys, json, urllib.request
downloads = json.loads(sys.stdin.read())
for d in downloads:
    urllib.request.urlretrieve(d['url'], d['path'])
    print('Downloaded ' + d['name'] + ' -> ' + d['path'])
" 2>&1) || {{
        echo "ERROR: Failed to download inputs" >&2
        echo "$DOWNLOADS" >&2
        exit 1
    }}
    echo "$DOWNLOADS"
    unset OZZY_INPUT_DOWNLOADS
fi

# Ensure output directory exists
mkdir -p /workspace/output

# Run the transform
{runner_cmd}

# Verify output was produced
if [ -z "$(ls -A /workspace/output 2>/dev/null)" ]; then
    echo "ERROR: Transform produced no output in /workspace/output/" >&2
    exit 1
fi

# Upload output tarball to presigned URL
cd /workspace/output
tar czf /tmp/output.tar.gz .
UPLOAD_URL=$(cat /tmp/ozzy_upload_url)
rm -f /tmp/ozzy_upload_url
curl -sSf -X PUT -T /tmp/output.tar.gz "$UPLOAD_URL" || {{
    echo "ERROR: Failed to upload output" >&2
    exit 1
}}

echo "OzzyDB init: transform completed successfully"
"#,
        runner_ext = runner_ext,
        runner_cmd = runner_cmd,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_python() {
        let script = generate_init(RunnerType::Python);
        assert!(script.starts_with("#!/bin/sh"));
        assert!(script.contains("OZZY_RUNNER_SCRIPT_B64"));
        assert!(script.contains("base64 -d > /workspace/runner.py"));
        assert!(script.contains("OZZY_INPUT_DOWNLOADS"));
        assert!(script.contains("python3 /workspace/runner.py"));
        assert!(script.contains("ozzy_upload_url"));
    }

    #[test]
    fn test_init_r() {
        let script = generate_init(RunnerType::R);
        assert!(script.contains("base64 -d > /workspace/runner.R"));
        assert!(script.contains("Rscript /workspace/runner.R"));
    }

    #[test]
    fn test_init_command() {
        let script = generate_init(RunnerType::Command);
        assert!(script.contains("base64 -d > /workspace/runner.sh"));
        assert!(script.contains("/bin/sh /workspace/runner.sh"));
    }

    #[test]
    fn test_init_loads_secrets() {
        let script = generate_init(RunnerType::Python);
        assert!(script.contains("OZZY_SECRETS_URL"));
        assert!(script.contains("/tmp/secrets.env"));
        assert!(script.contains("urllib.request"));
    }

    #[test]
    fn test_init_has_output_verification() {
        let script = generate_init(RunnerType::Python);
        assert!(script.contains("ls -A /workspace/output"));
    }

    #[test]
    fn test_init_uploads_output() {
        let script = generate_init(RunnerType::Python);
        assert!(script.contains("tar czf"));
        assert!(script.contains("curl"));
    }

    #[test]
    fn test_init_unsets_sensitive_vars() {
        let script = generate_init(RunnerType::Python);
        assert!(script.contains("unset OZZY_RUNNER_SCRIPT_B64"));
        assert!(script.contains("unset OZZY_INIT_SCRIPT_B64"));
        assert!(script.contains("unset OZZY_SECRETS_URL"));
        assert!(script.contains("unset OZZY_OUTPUT_UPLOAD_URL"));
    }

    #[test]
    fn test_init_downloads_source() {
        let script = generate_init(RunnerType::Python);
        assert!(script.contains("OZZY_SOURCE_DOWNLOAD"));
        assert!(script.contains("/workspace/source"));
        assert!(script.contains("source.tar.gz"));
        assert!(script.contains("curl -sS -o /tmp/source.tar.gz"));
        assert!(script.contains("unset OZZY_SOURCE_DOWNLOAD"));
    }

    #[test]
    fn test_init_conditional_downloads() {
        let script = generate_init(RunnerType::Python);
        assert!(script.contains("if [ -n \"$OZZY_INPUT_DOWNLOADS\" ]"));
    }
}
