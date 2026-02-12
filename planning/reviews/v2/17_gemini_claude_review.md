# Phase 3 Review Round 1 (Gemini + Claude)

## Reviewers
- Gemini (primary review)
- Claude Opus 4.6 (parallel sub-agent review)

## Findings (Deduplicated)

### Critical
1. **Push `get_or_create_project` uses collaborator's user_id** (push.rs:110-113)
   - Collaborator pushes would create phantom duplicate project under wrong owner
   - Fixed: Look up actual project owner by username before `get_or_create_project`

### High
2. **Push source path includes `:function_name`, will 404 on GitHub API** (push.rs:180-202)
   - `transforms/qc.py:quality_control` passed as file path, GitHub API rejects `:` in path
   - Fixed: `rsplit_once(':')` to strip function selector before API call

### Medium
3. **Commit count capped at 100** (projects.rs:115-122)
   - Used `list_commits(100).len()` as proxy for count
   - Fixed: Added `count_commits()` query with `SELECT COUNT(*)`

4. **Login poll loop has no timeout** (auth.rs:202-246)
   - Would hang forever if user never completes device flow
   - Fixed: Track `expires_in` from device code response, exit after deadline

5. **Webhook signature bypass when secret not configured** (webhooks.rs:47-63)
   - Webhooks processed without verification if no secret set
   - Fixed: Reject webhooks with 500 error when secret not configured

6. **No size limit on fetched tarballs** (github.rs:116)
   - `fetch_archive` loads entire tarball into memory without limit
   - Fixed: Added 500MB size check (both Content-Length header and actual bytes)

7. **SHA validation accepts uppercase** (push.rs:54-56)
   - `is_ascii_hexdigit()` accepts A-F, could cause case-mismatch in storage keys
   - Fixed: Normalize SHA to lowercase after validation

8. **Path not URL-encoded in GitHub API calls** (github.rs:142-145)
   - Special chars in path could break URL construction
   - Fixed: Added `urlencoding` dep, encode path segments individually

### Low
9. **`.gitignore` check uses substring match** (init.rs:256-260)
   - `existing.contains(entry)` could false-match substrings
   - Fixed: Line-by-line comparison instead

### Noted but not fixed (by design or deferred)
- No DB transaction wrapping push operations (would require refactoring DB layer)
- Mermaid rendering doesn't escape values (mitigated by name validation in toml_spec.rs)
- Installation token caching (optimization, not a bug)
- Redundant file fetches during push (optimization, not a bug)
- Test RSA private key embedded in source (test-only, won't trigger in prod)
