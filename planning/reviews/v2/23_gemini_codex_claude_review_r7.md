# Phase 3 Review Round 7 (Gemini + Codex + Claude)

## Reviewers
- Gemini (primary review via CLI)
- Codex (parallel review)
- Claude Sonnet 4.5 (parallel sub-agent review)

## Findings

### Fixed
1. **MEDIUM (Codex): Unbounded token expiration can panic** (auth.rs:265)
   - `expires_in_days` as `u32` added to `Utc::now()` via `chrono::Duration::days()` — overflow panics the handler
   - Fixed: Cap `expires_in_days` at 3650 (10 years), return 400 Bad Request for larger values

2. **MEDIUM (Codex): Scaffold generates invalid Python/R for dashed names** (transform.rs:35,81)
   - Names like `my-transform` pass validation but `def my-transform(...)` is invalid Python syntax
   - Fixed: Convert dashes to underscores in generated function names; file names keep original dashes

3. **MEDIUM (Codex): Flatten on yanked root collection returns 200 []** (collections.rs:447)
   - The flatten endpoint checked existence but not yanked status before recursing
   - Fixed: Check yanked on root collection, return 410 Gone

### Dismissed
- Codex LOW: Private repo/no-installation misreported as missing `ozzy.toml` — Known GitHub API limitation (404 for both missing files and inaccessible repos), not actionable
- **Gemini: Clean round — no new findings**
- **Claude Sonnet: Clean round — no new findings**
