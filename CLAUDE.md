# OzzyDB

## Slack

Slack channel ID for notifications: `C0AEMR991TQ`

## Project Structure

Cargo workspace with three crates + frontend + Python client:

- `crates/ozzy-core` — Core library (hashing, platform, canonicalization, schema, TOML parser)
- `crates/ozzy-cli` — CLI binary (clap)
- `crates/ozzy-server` — Registry server (Axum 0.8, PostgreSQL/sqlx, R2 storage)
- `frontend/` — SvelteKit 5 SPA (`@sveltejs/adapter-static`)
- `clients/python/` — Python client library (`ozzydb`)

## Architecture

v2 branch is canonical. See `planning/v2_architecture.md` and `planning/v2_implementation_details.md`.

OzzyDB is a switchboard: Git owns code, container registries own environments, OzzyDB owns data + orchestration + caching. Two planes: imperative data plane (upload/collections/yank) and declarative compute plane (`ozzy.toml` in git).

## Key Conventions

- BLAKE3 for all hashing
- Content-addressed storage throughout
- `ozzy.toml` is the declarative heart of the compute plane
- Deterministic execution: `PYTHONHASHSEED=0`, `OMP_NUM_THREADS=1`, etc.
- Names must match `[a-zA-Z0-9_-]` — no dots, colons, slashes, whitespace

## Testing

- `just test` — unit + non-Docker integration tests
- `just test-docker` — Docker integration tests
- `just test-e2e` — end-to-end tests
- `just test-all` — everything

## Deployment

- VPS: Hetzner CX22 at `46.225.111.110` (`ssh root@ozzydb` via Tailscale)
- `api.ozzydb.com` → Axum server, `ozzydb.com` → static frontend
- Docker Compose: postgres:17 + caddy:2 + ozzy-server
- After frontend rebuild, restart Caddy (`docker compose restart caddy`)

## Active Workflow (v2 Implementation)

You are in the middle of implementing the v2 architecture. Before doing anything else:

1. Read `planning/v2/WORKFLOW_STATE.md` for current progress (phase, step, what's done, what's next)
2. Read `planning/v2/implementation_plan.md` for the full plan
3. Read `planning/v2_architecture.md` and `planning/v2_implementation_details.md` for the spec
4. Read `planning/AGENT_WHITEBOARD.md` for observations from previous sessions
5. Read `.claude/napkin.md` for mistakes and patterns learned

Follow the riley-skills **workflow** protocol:
- Implement step by step, atomic commits after each meaningful unit of work
- Write tests, run tests, iterate until green
- After completing a step, run a review (riley-skills **review** skill)
- At phase milestones, run exhaustive review loops (2 consecutive clean rounds)
- File review artifacts in `planning/reviews/v2/`
- Update `planning/v2/WORKFLOW_STATE.md` after every step
- Notify via Slack (`C0AEMR991TQ`) for milestones, blockers, and design decisions — use `slack_notify` for status updates and `slack_ask` when you need a human response
- Append observations to `planning/AGENT_WHITEBOARD.md`
