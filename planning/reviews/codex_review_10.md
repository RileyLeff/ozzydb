# Round 10 Code Review (2026-02-07)

## Reviewers
- Gemini 2.5 Pro (3 findings)
- Claude Opus 4.6 (10 findings)

## Findings (Deduplicated: 11 unique)

### Fixed

| # | Severity | Source | Finding | File | Fix |
|---|----------|--------|---------|------|-----|
| 1 | HIGH | Opus | Tag upserts outside transaction boundary - errors after commit cause misleading error response | push_pull.rs:422-445 | Made tag processing truly best-effort with continue on errors |
| 2 | MEDIUM | Opus | TOML injection in setup_temp_project via unescaped project_name/owner | fetch.rs:206-209 | Escape backslashes and quotes before interpolation |
| 3 | MEDIUM | Opus | Non-atomic file writes in content storage - crash leaves corrupted partial files | content.rs:177-179 | Write to .tmp then atomically rename |
| 4 | MEDIUM | Gemini | dev-watch justfile doesn't source env file | justfile:40-42 | Added source docker/.env before cargo watch |
| 5 | LOW | Opus | revoke_token silently succeeds for non-existent tokens | auth.rs:191 | Check delete_token_by_name return value, return 404 |
| 6 | LOW | Opus | Lockfile hash not verified against commit metadata | push_pull.rs:343-349 | Added existence check for declared lockfile_hash |
| 7 | LOW | Opus | Owner/project names in URL paths not validated (path injection) | client.rs | Added validate_path_segment + project_url helper |

### Skipped (by design or repeat)

| # | Severity | Source | Finding | Reason |
|---|----------|--------|---------|--------|
| 8 | MEDIUM | Opus | get_stream doesn't verify content hash | Repeat from R8/R9 - design trade-off (streaming vs integrity) |
| 9 | MEDIUM | Opus | Ref name/type collision (tag shadows branch) | By design (Git parity) |
| 10 | MEDIUM | Opus | check_content extension mismatch | Data files are always parquet in practice |
| 11 | LOW | Opus | Unbounded multipart field name length | Axum/hyper has internal header size limits |

### False Positives

| # | Source | Finding | Reason |
|---|--------|---------|--------|
| 12 | Gemini | Inconsistent decorator parsing (" @ozzy.transform" vs "@ozzy.transform") | Code at line 182 uses trim() before the check - no leading space |
| 13 | Gemini | LocalCache race condition in access_count | SQLite is single-writer; not a real concurrent issue for CLI tool |
