# v4 Workflow State

## Current Phase: v4 Planning
## Current Step: Architecture, implementation plan, and type/publication addendum drafted, awaiting review

## Completed Steps

### Step 1: Create v4 planning scaffold
- Created `planning/v4/architecture.md`
- Created `planning/v4/AGENT_WHITEBOARD.md`
- Created `planning/v4/WORKFLOW_STATE.md`

### Step 2: Record current architectural decisions
- Locked the six primitive objects for v4.
- Recorded the current sanctioned relation set.
- Added product/record types and collection types to the assumed type language.
- Documented the current verification model and type/environment/provider identity split.

### Step 3: Draft v4 implementation plan
- Created `planning/v4/implementation_plan.md`.
- Put the v3-to-v4 deletion/migration matrix near the front.
- Committed to a no-backwards-compatibility, API-first execution order.
- Sequenced the plan around new primitives first, then ingestion, artifacts, execution, API, clients, and finally deletion sweep.

### Step 4: Tighten the v1 semantic rules
- Added a v1 type-language addendum to `planning/v4/architecture.md`.
- Pinned grammar, open/closed record syntax, omitted-argument semantics, refinement rules, and bottom-type behavior.
- Added a publication-model addendum covering `PublicationBundle`, atomic publication, and version conflict rules.
- Updated `planning/v4/implementation_plan.md` so Phase 1 and Phase 2 explicitly target those rules and include `Invocation` persistence.

## Current Blockers
- v4 architecture and implementation plan have not yet been reviewed together.
- No implementation work should begin until the first execution phase is explicitly accepted.

## Next Recommended Steps
1. Review `planning/v4/architecture.md` and `planning/v4/implementation_plan.md` together for ontology drift.
2. Confirm the v1 semantic rules are sufficient for Phase 1 (`ozzy-types`) without another design pass.
3. If accepted, start implementation at Phase 1.1 (`crates/ozzy-types`).

## Notes
- Existing v3 planning and type-system notes remain as background context and have not been rewritten.
- Unrelated working-tree changes under `clients/python/uv.lock` and untracked v3 type-note files were left untouched.
- Frontend planning remains intentionally deferred.
