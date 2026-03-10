# OzzyDB

## Current Planning Baseline

The active planning baseline is v4.

Read these first before implementing:

1. `planning/v4/architecture.md`
2. `planning/v4/implementation_plan.md`
3. `planning/v4/WORKFLOW_STATE.md`
4. `planning/v4/AGENT_WHITEBOARD.md`

The older v3 planning docs are background context only unless a v4 document explicitly points back to them.

## Working Rules

- No backwards-compatibility shims unless a v4 planning document explicitly requires one.
- API first. Server contract and internal model come before CLI and frontend work.
- Delete dead abstractions early. Do not preserve fake generality out of habit.
- Prefer replacing the v3 control plane cleanly over layering v4 semantics on top of it.

## Error Handling

This project does not permit silent fallback paths in core semantic code.

- Errors are data and should be represented explicitly with Rust error types.
- Do not hide failures behind defaults, degraded modes, warning-and-continue behavior, or best-effort recovery unless the architecture explicitly requires that fallback.
- Do not use `Option` to suppress real failures.
- Do not use `unwrap_or`, `unwrap_or_default`, or similar fallback behavior in parsing, canonicalization, refinement checking, verification, publication, registry snapshot loading, or execution-planning paths unless the fallback is explicitly intended and documented.
- Prefer specific failure over ambiguous success.
- Any intentional fallback must be called out in code comments and in the relevant planning document.

In practice:

- failed parsing should fail parsing
- failed verification should be recorded explicitly, not silently downgraded
- failed publication should roll back atomically
- missing type or transform references should error, not degrade to broader types like `bytes`

## Rust Guidance

- Prefer domain error enums for real subsystem boundaries.
- Use `anyhow` at app/glue boundaries, not as a substitute for modeling semantic errors.
- Keep the type system implementation explicit and data-driven: concrete ASTs, canonical forms, structured witnesses.
- Avoid hidden control flow in semantic code. Make invariants and failure modes visible.

## Project Structure

- `crates/ozzy-types` — planned v4 type-system crate
- `crates/ozzy-core` — shared core utilities
- `crates/ozzy-server` — Axum server, DB, orchestration
- `crates/ozzy-cli` — CLI
- `clients/python/` — Python client
- `frontend/` — deferred relative to the v4 server/API rewrite

## Testing

- `just test`
- `just test-docker`
- `just test-e2e`
- `just test-all`

## Notes

- Frontend work is deferred unless the user explicitly asks for it.
- If a planning document and old code disagree, the planning document wins.
