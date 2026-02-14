//! Init script generation — the container entrypoint.
//!
//! The init script is the first thing that runs inside the compute container.
//! For Docker-based execution (local or server), inputs are bind-mounted so
//! no download is needed. The init script:
//! 1. Decodes the runner script from an env var (or finds it at a known path)
//! 2. Runs the transform command
//! 3. Verifies output exists

use super::RunnerType;

/// Generate an init script for Docker bind-mount execution.
///
/// In this mode, inputs are already available at /workspace/inputs/ via bind mounts,
/// and the runner script is written to /workspace/runner.{ext} by the orchestrator.
pub fn generate_docker_init(runner_type: RunnerType) -> String {
    let runner_cmd = match runner_type {
        RunnerType::Python => "python3 /workspace/runner.py",
        RunnerType::R => "Rscript /workspace/runner.R",
        RunnerType::Command => "/bin/sh /workspace/runner.sh",
    };

    format!(
        r#"#!/bin/sh
set -e

echo "OzzyDB init: starting transform execution"

# Ensure output directory exists
mkdir -p /workspace/output

# Run the transform
{runner_cmd}

# Verify output was produced
if [ -z "$(ls -A /workspace/output 2>/dev/null)" ]; then
    echo "ERROR: Transform produced no output in /workspace/output/" >&2
    exit 1
fi

echo "OzzyDB init: transform completed successfully"
"#,
        runner_cmd = runner_cmd,
    )
}

/// Generate an init script for Fly Machines execution.
///
/// In this mode, inputs must be downloaded from presigned URLs and the runner
/// script is base64-encoded in an env var.
pub fn generate_fly_init(runner_type: RunnerType) -> String {
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

echo "OzzyDB init: starting (Fly mode)"

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

# Download inputs from presigned URLs
mkdir -p /workspace/inputs
DOWNLOADS=$(echo "$OZZY_INPUT_DOWNLOADS" | python3 -c "
import sys, json, urllib.request
downloads = json.loads(sys.stdin.read())
for d in downloads:
    urllib.request.urlretrieve(d['url'], d['path'])
    print(f\"Downloaded {{d['name']}} -> {{d['path']}}\")
" 2>&1) || {{
    echo "ERROR: Failed to download inputs" >&2
    echo "$DOWNLOADS" >&2
    exit 1
}}
echo "$DOWNLOADS"

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
curl -s -X PUT -T /tmp/output.tar.gz "$OZZY_OUTPUT_UPLOAD_URL" || {{
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
    fn test_docker_init_python() {
        let script = generate_docker_init(RunnerType::Python);
        assert!(script.starts_with("#!/bin/sh"));
        assert!(script.contains("python3 /workspace/runner.py"));
        assert!(script.contains("mkdir -p /workspace/output"));
        assert!(script.contains("ls -A /workspace/output"));
    }

    #[test]
    fn test_docker_init_r() {
        let script = generate_docker_init(RunnerType::R);
        assert!(script.contains("Rscript /workspace/runner.R"));
    }

    #[test]
    fn test_docker_init_command() {
        let script = generate_docker_init(RunnerType::Command);
        assert!(script.contains("/bin/sh /workspace/runner.sh"));
    }

    #[test]
    fn test_fly_init_python() {
        let script = generate_fly_init(RunnerType::Python);
        assert!(script.contains("OZZY_RUNNER_SCRIPT_B64"));
        assert!(script.contains("base64 -d > /workspace/runner.py"));
        assert!(script.contains("OZZY_INPUT_DOWNLOADS"));
        assert!(script.contains("python3 /workspace/runner.py"));
        assert!(script.contains("OZZY_OUTPUT_UPLOAD_URL"));
    }

    #[test]
    fn test_fly_init_r() {
        let script = generate_fly_init(RunnerType::R);
        assert!(script.contains("base64 -d > /workspace/runner.R"));
        assert!(script.contains("Rscript /workspace/runner.R"));
    }

    #[test]
    fn test_fly_init_loads_secrets() {
        let script = generate_fly_init(RunnerType::Python);
        assert!(script.contains("OZZY_SECRETS_URL"));
        assert!(script.contains("/tmp/secrets.env"));
        assert!(script.contains("urllib.request"));
    }

    #[test]
    fn test_fly_init_has_output_verification() {
        let script = generate_fly_init(RunnerType::Python);
        assert!(script.contains("ls -A /workspace/output"));
    }

    #[test]
    fn test_fly_init_uploads_output() {
        let script = generate_fly_init(RunnerType::Python);
        assert!(script.contains("tar czf"));
        assert!(script.contains("curl"));
    }
}
