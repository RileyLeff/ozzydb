# Phase 3 Review Round 4 (Gemini + Claude)

## Reviewers
- Gemini (primary review via CLI)
- Claude Sonnet 4.5 (parallel sub-agent review)

## Findings

### Fixed
1. **CRITICAL (Gemini): Source storage path mismatch** (push.rs:343)
   - `r2_key` manually constructed as `source/{sha}.tar.gz` but ContentStorage stores at sharded `source/{sha[0:2]}/{sha[2:4]}/{sha}.tar.gz`
   - Fixed: Use `source_storage.storage_key()` to get the actual sharded path

2. **HIGH (Claude): Idempotent push doesn't update ref** (push.rs:159-172)
   - Duplicate SHA detection returned early without updating the ref
   - Push same SHA with different ref → ref never created
   - Fixed: Added `upsert_ref` in the idempotent return path

3. **MEDIUM (Gemini): validate_content_hash allowed uppercase** (content.rs:39)
   - Uppercase hex was accepted, creating case-mismatch risk on case-sensitive filesystems
   - Fixed: `!c.is_ascii_uppercase()` check added

4. **LOW (Gemini): Commit message no length limit** (push.rs)
   - No validation on message string length
   - Fixed: 10,000 character max

5. **LOW (Gemini): GitLab init generates unsupported config** (init.rs)
   - ozzy init detects GitLab but server rejects non-GitHub
   - Fixed: Warning message when provider != "github"

6. **LOW (Gemini): Auth poll sleep ordering** (auth.rs:203-208)
   - Sleep happened before deadline check → could overshoot
   - Fixed: Check deadline before sleeping

### Dismissed
- Gemini HIGH: SSH remote `"git @"` with space — Repeat false positive (code is correct)
- Gemini MEDIUM: collection_hash ignores member names — By design
- Gemini LOW: Flatten collection DAG visited set — Already dismissed
- Claude HIGH: get_stream() hash bypass from remote — Known trade-off
- Claude MEDIUM: Endpoint node validation — Handled by ozzy_toml.validate()
- Claude MEDIUM: Webhook secret mandatory — Intentional security fix
- Claude MEDIUM: Source path traversal — GitHub API rejects + toml_spec validates
- Claude LOW: Git remote parsing edge cases — Minor, not worth fixing
- Claude LOW: Timestamp on idempotent push — Correct behavior
