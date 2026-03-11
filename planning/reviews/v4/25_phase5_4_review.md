# Phase 5.4 Review

## Scope
- remove public compute-provider introspection from the v1 API
- keep provider selection internal to server infrastructure
- collapse runtime backend resolution to the server-selected backend only

## Self-review
- Deleted the public `/v1/compute/providers` route and its handler.
- Removed `compute` from the v1 API router, so provider listing is no longer part of the public contract.
- Replaced `ComputeRegistry::resolve(Option<&str>)` with `ComputeRegistry::backend()`, which only exposes the server-selected backend to the runtime path.
- Renamed internal diagnostic helpers to make their purpose explicit:
  - `configured_backend_names()`
  - `selected_backend_name()`
- Updated the orchestrator to use only the server-selected backend.
- Kept backend realization details available internally for startup logging and Fly orphan cleanup.

## Checks run
- `cargo fmt`
- `cargo check -p ozzy-server --tests`

## Targeted test note
- I attempted a focused `cargo test -p ozzy-server compute::mod::tests` pass, but it fell back into the usual non-informative compile/link behavior in this harness.
- The broader server check above completed successfully after clearing stale cargo processes.

## Issues found
- No new correctness issues remained after the final pass.
- Existing warning noise in older server tests remains unchanged and out of scope for this slice.
