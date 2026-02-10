# Round 16 Code Review (2026-02-07)

## Reviewers
- Codex gpt-5.3-codex xhigh (5 findings)
- Claude Opus 4.6 (7 findings, 1 withdrawn)
- Gemini default (10 findings, 5 false positives)

## Findings (Deduplicated: 16 unique, 5 fixed)

### Fixed

| # | Severity | Source | Finding | File(s) | Fix |
|---|----------|--------|---------|---------|-----|
| 1 | HIGH | Codex+Opus+Gemini | Commit timestamp lost in DB - pull reconstructs from created_at, breaking hash consistency | queries.rs, push_pull.rs, models.rs | Added `commit_timestamp TEXT` column, store original RFC3339 timestamp, return in pull response |
| 2 | HIGH | Codex | Pull dirty check only compares (source_path, hash), misses lockfile_hash and other metadata | pull.rs | Compare full transform objects instead of just path+hash tuples |
| 3 | HIGH | Codex | Push rollback deletes content-addressed blobs that concurrent pushes may reference | push_pull.rs | Removed blob deletion from error paths, rely on GC for orphans |
| 4 | MEDIUM | Opus | DAG cycle silently falls back to insertion order instead of erroring | run.rs, fetch.rs | Changed build_execution_order to return Result, bail on cycle |
| 5 | MEDIUM | Codex | checked_destination creates dirs before validating ancestor path (symlink traversal) | pull.rs, fetch.rs | Walk up to nearest existing ancestor and validate before create_dir_all |

### Skipped (false positives, design concerns, or low risk)

| # | Severity | Source | Finding | Reason |
|---|----------|--------|---------|--------|
| 6 | CRITICAL | Gemini | Invalid Axum route syntax `{endpoint}@{ref}` | FALSE POSITIVE - matchit supports `@` literal separators between captures |
| 7 | CRITICAL | Gemini | Transform hash storage mismatch (canonical vs raw) | Server correctly rejects on mismatch, not silent corruption |
| 8 | HIGH | Gemini | RegistryClient URL space injection | FALSE POSITIVE - URL uses `@` not spaces |
| 9 | HIGH | Gemini | Client-side pull integrity bypass (no content re-verification) | Overlaps with fix #1; server now preserves original timestamp |
| 10 | HIGH | Gemini | Incomplete ref_count for deduplicated content | Latent, same category as "no GC yet - by design" |
| 11 | HIGH | Gemini | Ambiguous hashing with null bytes in hash_source_directory | Theoretical - null bytes don't occur in file paths or Python source |
| 12 | HIGH | Gemini | Fetch parsing off-by-one error | FALSE POSITIVE - code correctly uses find('@') + 1 |
| 13 | MEDIUM | Gemini+prev | Destructive pruning of unversioned files in pull | Already noted in R15 skipped (#6) |
| 14 | MEDIUM | Gemini | Shadowing in generated Params class if param named "get" | Edge case, very unlikely param name |
| 15 | MEDIUM | Opus | Content-addressed storage store() temp file collision | Already noted in R15 skipped (#8) |
| 16 | LOW | Opus+Gemini | Various low-severity edge cases (data source path, nocache collision, invalid parquet) | Low risk |

### Notes
- All 3 reviewers agreed on the commit timestamp issue (#1) - strong signal
- Gemini had 5 false positives this round (CRITICAL findings about Axum routes and URL construction were incorrect)
- Codex and Opus had no false positives
- 126 tests passing (108 Rust + 18 Python)
