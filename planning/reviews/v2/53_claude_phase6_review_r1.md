# Phase 6 Review Round 1 — Claude

**Date:** 2026-02-13
**Model:** Claude Opus (subagent)
**Scope:** Phase 6 Python client (types, http, client, tests)

## Findings

### Fixed

1. **HIGH** `fetch_lazy` permanent temp file leak — parquet path creates temp file, returns LazyFrame, never cleans up
   - Fixed: added `atexit.register()` cleanup handler for parquet temp files

2. **HIGH** `fetch_lazy` stream error temp file leak — if `iter_content()` fails, temp file is orphaned
   - Fixed: wrapped streaming in try/finally with cleanup on error

3. **MEDIUM** `_from_dict` fallback crashes on extra JSON keys — server adding new fields would break client
   - Fixed: filter dict keys to known dataclass fields via `dataclasses.fields()`

4. **MEDIUM** `download()` unnecessary `stream=True` — `.content` access defeats streaming purpose
   - Fixed: removed `stream=True` from download request

5. **MEDIUM** `run()` temp file has no extension, breaks text format detection (CSV/JSON/TSV)
   - Fixed: added text format heuristics to `_infer_content_type` (UTF-8 check + comma/tab/JSON detection)

6. **MEDIUM** Global singleton not thread-safe — TOCTOU race on `_default_client`
   - Fixed: added `threading.Lock()` with double-checked locking

7. **MEDIUM** `OzzyClient._session` never closed — connection pool leak with multiple instances
   - Fixed: added `close()` method + context manager protocol (`__enter__`/`__exit__`)

8. **LOW** Empty ref components not validated — `"alice//endpoint"` parses as `("alice", "", "endpoint")`
   - Fixed: added `not all(parts)` check to reject empty components

9. **LOW** `CREDENTIALS_PATH` evaluated at import time — doesn't respect HOME env var changes
   - Fixed: changed to `_credentials_path()` staticmethod computed at call time

### Dismissed

- GeoJSON gets `.json` extension — acceptable, JSON parser handles both
- Test coverage gaps (fetch_lazy non-parquet, download_dataframe as_pandas) — not critical for correctness

## Status: 9 fixes applied, 47 tests passing
