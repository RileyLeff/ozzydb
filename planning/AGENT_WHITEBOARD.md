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
