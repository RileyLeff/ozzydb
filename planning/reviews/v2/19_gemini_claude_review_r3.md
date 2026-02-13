# Phase 3 Review Round 3 (Gemini + Claude)

## Reviewers
- Gemini (primary review via CLI)
- Claude Sonnet 4.5 (parallel sub-agent review)

## Findings

### Dismissed
1. **Gemini CRITICAL: "Hardcoded BLAKE3 length prevents caching Git SHA-1"** — Downgraded to HIGH. Push itself succeeds; source caching failure is warned but not fatal.
2. **Gemini HIGH: Yanked check logic error in flatten_collection** — Invalid. Data atom names are unique per project; the name→hash mapping is 1:1.
3. **Gemini HIGH: Collection "Add" ignores hash updates** — By design. Collections pin specific versions at add time.
4. **Gemini MEDIUM: SSH Remote Detection has space** — False positive. Code correctly uses `"git@"` (no space).
5. **Gemini MEDIUM: Collection DAG visited set** — Acceptable. Prevents duplicate atoms in flattened output.
6. **Gemini MEDIUM: Non-Atomic Data Upload** — Phase 2 concern, recoverable (retry collection add). Deferred.
7. **Gemini LOW: Single-Session Restriction** — By design. Use API tokens for multi-machine access.

### Fixed
8. **HIGH: validate_content_hash rejects 40-char Git SHA** (content.rs:39-46)
   - Source tarball caching silently failed for all SHA-1 repos (40-char hash rejected by 64-char-only validation)
   - Fixed: Accept both 40 and 64 character hex hashes

9. **HIGH: Push DB operations not atomic** (push.rs:253-287)
   - `insert_commit`, `insert_commit_state`, `upsert_ref` were separate operations
   - Crash after `insert_commit` leaves dangling commit that blocks future pushes via idempotency check
   - Fixed: New `register_commit_atomically()` method wraps all three in a single transaction

10. **MEDIUM: Ref name missing character validation** (push.rs:108-112, from Claude sub-agent)
    - Ref names only checked for empty, `..`, leading `/` — allowed spaces, colons, etc.
    - Fixed: Added `chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '/')`
