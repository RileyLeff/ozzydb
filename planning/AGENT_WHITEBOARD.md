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
