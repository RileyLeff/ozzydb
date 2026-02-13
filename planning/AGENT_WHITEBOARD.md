# Agent Whiteboard

Chronological observations from agents working on OzzyDB.

---

**Claude Opus 4.6 — v2 Phase 1 — Pre-implementation**

- The current DB migrations (001-004) are v1-flavored. v2 needs a clean slate — single fresh migration replaces all four.
- `api_tokens.scopes` is currently `TEXT[]` (array). v2 spec says `scope TEXT` (singular: "account" or "project:{owner}/{slug}"). Auth middleware needs updating.
- `hash.rs` currently has `transform_hash()` without `environment_image_hash` param. v2 adds this.
- The CLI `main.rs` has all v2 subcommands enumerated but the command hierarchy matches v2 spec already (data upload/ls/show/describe/yank/download, collection create/add/rm/ls/log/flatten, etc.) — good scaffolding.
- `toml_spec.rs` doesn't exist yet — this is the most architecturally significant new module.
- Server `v1/access.rs` is empty (v1 project/push routes were removed). All v2 routes need to be built from scratch in the API layer.
- Frontend uses SvelteKit 2.50 / Svelte 5.49 / Vite 7.3 — current stack, no upgrades needed.

---

**Claude Opus 4.6 — v2 Phase 2 — Exhaustive review complete (2026-02-12)**

- Phase 2 data plane (data atoms, collections, secrets) implementation + exhaustive review finished.
- 6 review rounds: rounds 1-4 found real bugs, rounds 5-6 both clean → convergence.
- Key patterns established:
  - `CollectionMutResult<T>` enum for business-logic rejections vs internal errors in atomic DB ops
  - `pg_advisory_xact_lock(hashtext(project_id::text))` for serializing collection mutations per-project
  - Cycle detection runs on the transaction connection (not pool) to avoid deadlocks
  - Yanked checks happen both pre-lock (UX/early rejection) and inside lock (integrity)
  - `resolve_member_hash` rejects yanked members, `flatten_collection` skips them
- CLI integration tests are broken (pre-existing: `ozzy init` no longer takes `--name`/`--owner`)
- Codex rate limits are real (~240k token reviews). Gemini works as fallback with key-files-only approach (piped stdin + `-p` short prompt). Full dirgrab output causes exit code 13.
- Slack MCP server can disconnect between calls — may need retry logic or fallback to AskUserQuestion.

---

**Claude Opus 4.6 — v2 Phase 3 — Exhaustive review complete (2026-02-13)**

- Phase 3 (Git Integration & Push) implementation + exhaustive review finished.
- 11 review rounds total (across 3 sessions): rounds 1-8 found real bugs, rounds 9-11 clean → convergence.
- ~70 issues found and fixed total across Phases 2 + 3 reviews.
- Key findings pattern: most round 1-4 bugs were genuine (hash safety, auth scope escalation, path traversal). Later rounds found diminishing-return issues (UX polish, edge cases, design notes).
- Codex exhausted its usage credits during round 10, preventing further Codex reviews. Claude Haiku sub-agent proved reliable as a consistent reviewer.
- Gemini CLI failures (exit code 1, empty output) happened intermittently in rounds 7-8. Not reliable for automated review loops.
- CLI auth.rs had all 6 endpoints using wrong URL prefix (`/v1/` instead of `/api/v1/`) — caught in round 8 by both Codex and Claude independently.
- Phase 3 key components: GitHubProvider (JWT + installation tokens), push endpoint, endpoint inspection API, CLI init/scaffold, auth CLI commands.
- Pre-existing broken CLI integration tests still unresolved (31/33 fail due to `--name` flag removal).

---

**Claude Opus 4.6 — v2 Phase 4 — Exhaustive review complete (2026-02-13)**

- Phase 4 (Execution) implementation done (6 steps: environments, runners, compute, fetch, CLI run/fetch, cache).
- Review rounds 1-2 found 20 real issues, all fixed.
- Key findings:
  - Hash convergence: CLI and server computed different env hashes for BaseLockfile (CLI hashed the hash, server hashed raw content) and Prebuilt (server used raw string, CLI used blake3). Fixed both to match.
  - WorkspaceCleanup RAII using tokio::spawn in Drop was unsound — replaced with explicit async cleanup() method on ComputeResult.
  - generate_dockerfile used assert!() which would crash server process — changed to return Result.
  - validate_source_ref() added to prevent code injection via newlines in transform source references.
  - uv.lock handler was broken (uv.lock is TOML, not pip-installable) — removed, users should provide requirements.txt.
  - PlatformFingerprint::detect() called inside per-node loop — hoisted above.
  - Environment insert had TOCTOU race (check-then-insert without ON CONFLICT).
- Known TODOs in code (not review bugs): compute_inputs resolution, source_dir mounting, endpoint edge source resolution — these are incomplete features, not defects.
- Gemini CLI gotcha: `-p` flag + stdin doesn't work together. Use `gemini --sandbox -o text < file.txt` (stdin only).
- compute_env_hash changed from Option<String> to String — all tiers now produce a blake3 hash (Prebuilt included).
- Round 8 found only 1 low-severity issue (missed instance of round 7 temp file cleanup pattern in get() hydrate path). All 3 round 7 fixes verified present. Approaching convergence — need 2 consecutive clean rounds.
- 18 review rounds completed. Highlights from rounds 13-18:
  - Reserved param names (ref, format) and hyphen rejection for shell safety
  - Poetry.lock rejection (requires pyproject.toml, BaseLockfile tier can't support)
  - Push source cache failure now fatal for source-based transforms
  - Prebuilt environments synthesize DB records directly (no race with async builder)
  - Collection add hash-based dedup (DB has UNIQUE constraint, code must match)
  - Python runner switched from dotted imports to importlib (handles hyphens in paths)
  - Token creation access check (was leaking private project existence)
  - Push scope check moved before project creation (side-effect prevention)
  - Reserved runtime env var blocklist for secrets (PYTHONHASHSEED, PATH, etc.)
  - CLI path containment (ensure_within_dir for source/lockfile/dockerfile)
  - Failed env builds: pending rows cleaned up for retry on re-push
  - Docker timeout: child process now explicitly killed via `docker kill --name`
  - Terminal node detection: find_terminal_node() replaces fragile exec_order.last()
- Codex rate limited after round 18. Switched to Claude subagent reviews for rounds 19-21.
- Round 19 (Claude subagent): found 2 real bugs — container name mismatch in docker.rs timeout kill, lockfile hash divergence in fetch.rs transform_hash.
- Rounds 20+21 (Claude subagent): both clean → **convergence achieved**.
- 21 review rounds total, ~47 bugs fixed, 43 known limitations, 278 tests pass.
- Gemini CLI consistently fails with exit code 13 on ~260k token contexts. Claude-only reviews worked well for Phase 4.

---

**Claude Opus 4.6 — v2 Phase 5 — Exhaustive review complete (2026-02-13)**

- Phase 5 (Frontend) implementation done (8 steps: API client/types, project overview, data browser, collection browser, endpoint explorer, commit history, settings/secrets, user profile).
- 5 review rounds: rounds 1-3 found real bugs, rounds 4-5 both clean → convergence.
- 13 issues found and fixed total:
  - ProjectTabs `const` → `$derived` for reactivity on prop changes (Svelte 5 gotcha)
  - UUID→username resolution pattern applied across 3 server modules (data.rs, collections.rs, commits.rs) with `resolve_username()` helper
  - Object URL lifecycle: must revoke in BOTH `$effect` (route change) AND `handleRun` (repeated execution) paths
  - Stale state on navigation: collection detail needed to reset flattenedAtoms/showFlatten/flattenLoading in `$effect`
  - `fetchEndpoint` ref param collision: set `ref` after user params to prevent override
  - Negative limit clamping with `.max(1)` in commits API
  - Svelte 5 `{@const}` only valid inside block tags (`{#if}`, `{#each}`), not as direct children of elements
  - Retry button pattern: `retryCount` state variable subscribed in `$effect` — incrementing triggers re-fetch
- Codex rate-limited during round 3. Claude Opus subagent proved fully capable for all remaining review rounds.
- Key Svelte 5 patterns established:
  - `$effect` with snapshot capture (`const cur = { owner, slug }`) for race condition guarding
  - `$derived` for computed values that depend on props (not `const`)
  - State reset at top of `$effect` before async operations
  - Object URL revocation before overwriting references
- Frontend build: `npm run build` + `npm run check` both pass cleanly.
- Total: 278 backend tests + frontend TypeScript checks all passing.
