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
- `PlatformFingerprint::detect()` runs on server, not in container — **confirmed bug** (Gemini 2.5 review, 2026-02-23). Harmless in current single-server Docker setup but will produce wrong materialized cache keys on Fly or multi-arch. Detailed writeup in `planning/v3/next_steps.md` under "BUG: PlatformFingerprint". Fix blocked on compute provider decisions.
- Docker containers have network access to host (needed for presigned URL I/O) — consider custom network with limited egress for defense-in-depth.

## Session 2026-02-14: CLI Implementation + Review

### CLI Review Summary
- 8 rounds, 24 total fixes, Claude Opus only (~380k tokens exceeds Codex/Gemini limits)
- Key hardening: BLAKE3 hash verification on downloads, input name validation, HTTPS warnings, symlink safety, absolute URL handling
- Convergence pattern: fix count per round was 7→5→2→5→2→1→2→0 (monotone decrease with one bump at round 4)

### E2E Smoke Test Setup
- tryozzydb project scaffolded at `~/Documents/dev/try/tryozzydb/`
- Uses prebuilt `python:3.12-slim` image (no env build needed)
- Simple CSV→CSV transform (greet.py reads names, outputs greetings)

### E2E Smoke Test: Complete (2026-02-14)

Full pipeline working: `ozzy push` → `ozzy data upload` → `ozzy fetch` → correct output + cache hit

**Bugs found and fixed during smoke test:**
1. **Fly Machine event timestamps** — API returns integers (millis since epoch), not strings. Fixed with `#[serde(untagged)]` enum deserializer.
2. **Exit code interpretation** — `exit_code` is the actual command exit, `guest_exit_code` is Fly init system. Initially had them backwards.
3. **Missing /workspace/ directory** — Standard Docker images don't have it. Added `mkdir -p /workspace` to init script.
4. **No curl in python:3.12-slim** — Replaced all curl calls with Python `urllib.request` in init script.
5. **Storage key extension mismatch** — Upload stored with `.bin` extension, download looked for content-type extension (`.csv`). Fixed both paths to use `content_type_to_extension()`.
6. **Transform calling convention** — Test transform used old v2 `def greet()` signature. Updated to v3 `def greet(inputs, params)`.

**Key debugging tool:** `flyctl logs --app ozzydb-compute --no-tail` on VPS for container stdout/stderr.

**Cache verification:** Second fetch returns instantly with "Cache hit" (1s vs initial compute time).
