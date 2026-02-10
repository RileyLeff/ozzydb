# Round 15 Code Review (2026-02-07)

## Reviewers
- Codex gpt-5.3-codex xhigh (7 findings)
- Claude Opus 4.6 (5 findings)
- Gemini 3 Pro (FAILED - model not found)

## Findings (Deduplicated: 10 unique, 5 fixed)

### Fixed

| # | Severity | Source | Finding | File(s) | Fix |
|---|----------|--------|---------|---------|-----|
| 1 | HIGH | Codex+Opus | Transform hash missing function_name - cache collision for multiple transforms in same file | hash.rs, run.rs, fetch.rs | Added `function_name` parameter to `transform_hash()` |
| 2 | HIGH | Codex+Opus | JSON canonicalization doesn't escape object keys - hash collision risk | canon.rs | Extracted `canonicalize_json_string()` helper, used for both keys and values |
| 3 | HIGH | Codex+Opus | Duplicate transform names silently overwritten | commit.rs | Added `contains_key` check before insert, returns `TransformAlreadyExists` error |
| 4 | HIGH | Codex | Lockfile hash sentinel mismatch - blake3(b"") is not empty string, server rejects valid pushes | push_pull.rs | Compare against `blake3_hash(b"")` sentinel instead of `is_empty()` |
| 5 | MEDIUM | Codex+Opus | anyhow errors produce 500 instead of 404 for not-found resources | push_pull.rs | Replaced `anyhow::anyhow!("...not found")` with `ApiError::not_found(...)` |

### Skipped (design concerns, low risk, or need further analysis)

| # | Severity | Source | Finding | Reason |
|---|----------|--------|---------|--------|
| 6 | HIGH | Codex+Opus | Pull prunes untracked helper files | Architectural concern - needs design decision about tracked vs untracked files |
| 7 | MEDIUM | Codex+Opus | Pull strips explicit ref type (tag vs branch) | Need to verify actual behavior; low impact with current CLI |
| 8 | MEDIUM | Opus | Race condition in content.rs store() - deterministic tmp path | Server-side only, concurrent identical-hash stores produce correct result |
| 9 | MEDIUM | Opus | Non-atomic local cache hydration in get() | Low risk - hash verification catches corruption, just produces confusing error |
| 10 | MEDIUM | Opus | source_storage_key silent fallback in pull response | Masking potential data integrity issues, but not a security concern |

### Notes
- Gemini 3 Pro failed with `ModelNotFoundError` - model name may not be available yet in CLI
- Strong agreement between Codex and Opus on top 3 findings (transform hash, JSON key escaping, duplicate transforms)
- Lockfile sentinel mismatch (Codex #5) is a real bug that would cause push failures for transforms without uv.lock
