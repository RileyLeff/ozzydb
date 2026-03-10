# Phase 3.1 Review

## Scope

Phase 3.1 `ozzy.toml` ingestion rewrite.

Touched code:

- `crates/ozzy-core/Cargo.toml`
- `crates/ozzy-core/src/lib.rs`
- `crates/ozzy-core/src/toml_spec.rs`
- `crates/ozzy-server/src/registry.rs`
- `crates/ozzy-types/Cargo.toml`
- `crates/ozzy-types/src/lib.rs`
- `crates/ozzy-types/src/parse.rs`
- `crates/ozzy-types/src/schema.rs`
- `crates/ozzy-types/src/verify/witness.rs`

Deleted:

- `crates/ozzy-core/src/error.rs`
- `crates/ozzy-core/src/schema.rs`

## What Changed

1. Moved schema/witness support out of `ozzy-core` and into `ozzy-types`, removing the crate cycle blocker for typed authored definitions.
2. Added a minimal v1 parser for:
   - full type expressions
   - type references for transform ports
3. Rewrote `ozzy_core::toml_spec` around:
   - top-level `[types]`
   - typed `inputs` and `outputs`
   - removal of `output` and `output_schema`
4. Made port refs strict:
   - ports accept refs, not arbitrary inline type expressions
   - builtin refs cannot be version-pinned
5. Tightened the server snapshot binding path so authored transform bindings compare both port names and resolved published type identities.
6. Removed the old schema fallback behavior when moving witness parsing into `ozzy-types`; malformed stored schema types now error instead of silently degrading to `Utf8`.

## Review Findings

### Fixed during review

1. Builtin port refs could still be version-pinned (`parquet@1`) and slip through validation.
   - Fixed in `toml_spec.rs`
2. Registry binding only compared transform port names, not the bound published type identities.
   - Fixed in `registry.rs`

### External review status

- Gemini CLI produced MCP startup noise and unusable output for this checkpoint.
- Claude CLI did not produce a usable one-shot review output in this environment.
- Per `review_notes_README.md`, this checkpoint proceeded with explicit self-review plus tests instead of blocking on flaky reviewer processes.

## Verification

Executed:

- `cargo test -p ozzy-core`
- `cargo test -p ozzy-types`
- `cargo check -p ozzy-server --tests`

Notes:

- `ozzy-server` test compile still emits pre-existing warning noise in unrelated test files.

## Exit Assessment

Phase 3.1 is complete.

The authored project spec now matches the v4 direction materially better:

- typed local definitions at the top level
- typed transform ports instead of stringly `inputs/output/output_schema`
- no silent schema fallback in the witness layer
- no `ozzy-core`/`ozzy-types` dependency cycle

The next meaningful step is Phase 3.2: compile authored `ozzy.toml` into a `PublicationBundle` and publish first-class registry/project revision objects atomically.
