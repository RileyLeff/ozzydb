# Agent Whiteboard — v3.1 Implementation

## Session 2026-02-14: v3.1 Exhaustive Review

### Key Bugs Found & Fixed
1. **Secrets presigned URL used server-facing S3 client** — would fail in local dev (MinIO). Must always use `_for_compute` variants for any URL a container will access.
2. **Wave error handling dropped JoinHandles** — orphan compute tasks. Always await remaining handles before cleanup.
3. **Secrets R2 key per-job, not per-node** — parallel nodes in same wave would overwrite each other's secrets. Include node_name in all per-node R2 keys.
4. **Source tarball not cleaned on compute_waves() failure** — R2 leak on cycle detection. All error paths must clean up uploaded resources.

### Patterns Observed
- Gemini CLI consistently fails with E2BIG for this codebase (~377k tokens). Not usable for reviews at this scale.
- Codex hit usage limits. Claude opus subagent is the reliable fallback.
- The `tar` crate (0.4.44) has built-in path traversal protection — no need for manual validation on `unpack()`.
- `tokio::process::Command` doesn't use shell — env vars passed via `-e KEY=VALUE` are safe from injection.
- `ComputeBackend: Any` supertrait bound correctly preserves TypeId through trait object downcasting.

### Design Notes for Future
- Collections (`is_collection`) not wired in orchestrator — will need multi-file download support when implemented.
- `PlatformFingerprint::detect()` runs on server, not in container — fine for single-server Docker, needs rethinking for multi-arch Fly deployments.
- Docker containers have network access to host (needed for presigned URL I/O) — consider custom network with limited egress for defense-in-depth.
