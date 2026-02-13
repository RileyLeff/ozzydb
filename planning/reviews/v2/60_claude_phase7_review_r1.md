# Phase 7 Review Round 1 — Claude Opus

## Findings

| Severity | ID | Finding | Status |
|----------|----|---------|--------|
| H | H1 | Server vs CLI source_hash divergence (synthetic vs file hash) | Fixed |
| H | H2 | Empty compute_inputs — data inputs not mounted in containers | Known TODO |
| M | M1 | CLI Docker timeout does not kill orphan containers | Fixed |
| M | M2 | MaybeAuthUser silently swallows invalid/expired tokens | Fixed |
| M | M3 | New ContentStorage created per request instead of state.storage | Fixed |
| L | L1 | Synthetic hash for endpoint: edge references | Known TODO |

## Actions Taken

- **H1**: Server fetch.rs now hashes actual source file contents (for source transforms) or command string (for command transforms), matching CLI behavior. Falls back to synthetic hash when source tarball unavailable.
- **H2**: Skipped — known TODO, E2E tests use param-only transforms. Full data input hydration is a separate feature.
- **M1**: CLI run.rs names Docker containers (`ozzy-run-{pid}-{nanos}`) and kills them on timeout via `docker kill`.
- **M2**: MaybeAuthUser now returns 401 when Authorization header is present but token is invalid. Missing header still allows anonymous access.
- **M3**: Replaced `ContentStorage::from_config()` calls with `state.storage` in fetch.rs (2 instances).
- **L1**: Skipped — known TODO for cross-endpoint references.

## Test Results

- 13 E2E tests: all pass
- 14 integration tests: all pass
- Clean compilation, no warnings
