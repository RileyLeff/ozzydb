# Phase 4 Review Round 16 — Claude Opus

**Date:** 2026-02-13
**Model:** Claude Opus 4.6 via Codex
**Commit before:** `7b178de`
**Commit after:** `fac91a1`
**Tests:** 90 core + 109 server + 4 CLI unit = 203 pass

## Findings (5 total, 3 real bugs fixed, 2 design notes)

### Fixed

1. **HIGH — Push creates project before scope check** (`push.rs`)
   - `get_or_create_project` ran before `enforce_write_access`, so a push with a
     project-scoped token for a nonexistent project would create it as a side effect
   - Fix: Check scope/ownership first, only call `get_or_create_project` after
     confirming authorization

2. **MEDIUM — CLI Python runner uses dotted imports (parity)** (`run.rs`)
   - Server runner was updated to importlib in r15, but CLI's `generate_python_runner`
     still used `from X.Y import func` which breaks on hyphenated paths
   - Fix: Updated to importlib.util.spec_from_file_location, matching server
   - Updated test assertions to match

3. **MEDIUM — Secrets can override determinism env vars** (`fetch.rs`)
   - Secret names like `PYTHONHASHSEED`, `OMP_NUM_THREADS`, `PATH` etc. could
     silently override runtime control variables set by the compute backend
   - Fix: Added `RESERVED_SECRET_NAMES` blocklist with case-insensitive check;
     returns 400 Bad Request if a secret matches

### Design notes (known limitations)

4. **MEDIUM — Cache-miss execution has empty compute_inputs** (`fetch.rs`)
   - `compute_inputs: Vec<InputSpec> = Vec::new() // TODO` means cache-miss nodes
     will run with no input files mounted. Known limitation.
   - Added as known limitation #38.

5. **LOW — Streamed remote reads skip hash verification** (`storage/`)
   - When streaming content from R2/local storage, the response is not verified
     against the expected content hash. Only relevant for storage corruption;
     currently local-only storage.
   - Added as known limitation #39.

## Known Limitations Updated (39 total)

#38: Compute inputs not resolved to local paths (TODO in fetch.rs cache-miss path)
#39: Streamed content reads not verified against expected hash
