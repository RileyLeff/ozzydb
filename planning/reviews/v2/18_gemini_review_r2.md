# Phase 3 Review Round 2 (Gemini)

## Reviewer
- Gemini (via CLI)

## Findings

### Dismissed
1. **CRITICAL: "Configuration corruption in justfile"** — False positive. The justfile is intact and all recipes work correctly. Gemini likely confused file content with something else.

2. **HIGH: URL instability from username changes** — Already documented as a design note in `upsert_user_from_github` (queries.rs). Deferred to pre-launch.

### Fixed
3. **HIGH: Project-scoped token metadata leak in `list_projects`** (projects.rs:80-96)
   - `list_projects` didn't check `scope_grants_project_access`, allowing project-scoped tokens to list all private projects
   - Fixed: Added scope check inside the loop, `continue` on scope mismatch

4. **MEDIUM: Invalid slugs with slashes in push** (push.rs:73-75)
   - `split_once('/')` allowed slugs like `my/project` which break Axum routing
   - Fixed: Added `is_valid_name()` validation for both owner and slug

5. **MEDIUM: Yank race condition with collection add** (queries.rs)
   - `yank_collection` and `yank_data_atom` didn't acquire advisory lock
   - Fixed: Both now use `pg_advisory_xact_lock(hashtext(project_id))` in a transaction

### Deferred (optimization)
6. **LOW: Sequential file checks in push** — Parallelize with `join_all`. Optimization, not correctness.
7. **LOW: New reqwest::Client per call** — Reuse from AppState. Optimization, not correctness.
8. **LOW: 32-bit advisory lock hash** — Use 64-bit for less contention. Minor optimization.
