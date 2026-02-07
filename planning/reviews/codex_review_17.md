# Round 17 Code Review (2026-02-07)

## Reviewers
- Codex gpt-5.3-codex xhigh (4 findings)
- Claude Opus 4.6 (9 findings, 5 low/edge cases)
- Gemini default (5 findings, 2 overlap with prior rounds)

## Findings (Deduplicated: 10 unique, 6 fixed)

### Fixed

| # | Severity | Source | Finding | File(s) | Fix |
|---|----------|--------|---------|---------|-----|
| 1 | HIGH | Codex+Gemini | Server hashes transform uploads with raw bytes, not canonical - breaks cross-platform push | push_pull.rs | Canonicalize transform source before hashing/storing on server |
| 2 | HIGH | Codex | Pull updates HEAD when `display_ref == "main"` even when current HEAD is another branch | pull.rs | Remove `|| display_ref == "main"` condition |
| 3 | MEDIUM | Codex+Gemini | Pull/fetch extract files without verifying hashes against commit metadata | pull.rs, fetch.rs | Added hash verification for data sources and transforms after extraction |
| 4 | MEDIUM | Opus | content.rs get() hydration writes directly (crash leaves corrupted partial file) | content.rs | Write to temp file, then rename atomically |
| 5 | LOW | Codex+Opus | Credentials file created world-readable before chmod (race window) | auth.rs (CLI) | Use OpenOptions with mode 0o600 on Unix |
| 6 | LOW | Opus+Gemini | Auth token delete+create in github_poll not atomic (concurrent logins race) | auth.rs (server) | Wrapped in DB transaction |

### Skipped (false positives, already reported, or low risk)

| # | Severity | Source | Finding | Reason |
|---|----------|--------|---------|--------|
| 7 | CRITICAL | Gemini | Timestamp precision mismatch in commit hash | FALSE POSITIVE - R16 fix preserves DateTime value; roundtrip through serde produces identical hash |
| 8 | HIGH | Gemini | Memory exhaustion in pull/fetch (in-memory tar) | Already reported R13 #7, MAX_TAR_SIZE_BYTES limit exists |
| 9 | HIGH | Opus | prune_unlisted_files follows symlinks | FALSE POSITIVE - DirEntry::file_type() uses lstat on Unix, doesn't follow symlinks |
| 10 | MEDIUM-LOW | Opus | Username collision, token scope validation, negative offset, check_content extension | Edge cases / low risk |

### Notes
- Codex and Gemini independently found the transform canonicalization issue (#1) - strong signal
- The pull HEAD overwrite (#2) would silently corrupt branch state on multi-branch workflows
- Content hash verification (#3) closes the last gap in pull/fetch integrity checking
- Gemini had 1 false positive (timestamp precision); Opus had 1 false positive (symlink pruning)
- 126 tests passing (108 Rust + 18 Python)
