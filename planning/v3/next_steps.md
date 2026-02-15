# Next Steps

## Immediate (do now)

### GitHub App setup
Configure the existing GitHub App for private repo access. Currently only public repos work — the server logs `GitHub App not configured — only public repos accessible`.

### End-to-end smoke test
Push a real project and fetch an endpoint on production. The compute pipeline (push → job creation → DAG orchestration → Docker/Fly execution → R2 upload → cache hit) has only been tested in E2E tests with MinIO, never against real R2.

### Automated pg_dump to R2
Cron job: `pg_dump | gzip | upload to R2`. No backup strategy exists today.

### Rethink GitHub auth flow for LLM/CLI ergonomics
The current auth flow has high friction for LLM agents and automated workflows:
- `ozzy auth login` requires interactive browser-based GitHub device flow
- GitHub App installation requires visiting a web UI — no API/CLI path available
- Both steps block autonomous operation entirely

Consider alternatives:
- Personal access token auth (paste a PAT, skip device flow)
- Server-side API key auth (bypass GitHub entirely for CLI usage)
- `gh` CLI token reuse (detect existing GitHub auth)
- Make GitHub App optional (support public repos without it, allow PAT-based private repo access)

### Container initialization robustness
The current init script (shell bootstrap that runs inside compute containers) has fragile assumptions:
- Requires `/workspace/` to exist (fixed with `mkdir -p`, but other dirs may be missing)
- Requires `python3` for all HTTP I/O (downloads, uploads via urllib) — works for Python transforms but breaks for R/Julia/WASM
- Requires `base64` and `tar` to be available in the image
- Shell script embedded as base64 env var — hard to debug, no structured logging back to the server
- No way to stream logs from the container back to the server in real-time (only get exit code + events after the fact)

For v4, consider a statically-linked "ozzy-agent" binary injected into the container (via bind mount or sidecar). This would:
- Handle all presigned URL I/O without depending on Python/curl/wget
- Stream structured logs back to the server via a callback URL
- Create the workspace directory structure reliably
- Support any runtime (Python, R, Julia, WASM) without per-language init script variants

## Near-term optimizations

### Batch init for sequential same-environment nodes
Architecture spec (v3) describes grouping sequential DAG nodes that share an environment into a single machine dispatch, eliminating cold starts for chains like A → B → C. Currently each node gets its own machine. Meaningful cost/latency win for multi-step pipelines.

### Compute tier selection
Per-node `machine` field in ozzy.toml maps to named providers, but no tier differentiation within a provider (cpu-small vs cpu-large). Fly supports different `guest` configs.

### Per-project compute config in ozzy.toml
Let projects specify default machine/tier preferences.

## v4 (when demand exists)

### Managed Postgres
Neon, Supabase, or Fly Postgres. pg_dump to R2 is the interim.

### Multi-server redundancy
Single VPS is fine for now. Scale when needed.

### Job queue (Redis / Postgres SKIP LOCKED)
In-memory orchestration sufficient at current scale.

### Modal GPU integration
No GPU demand yet. ComputeBackend trait is ready for it.

### CI pipeline
`just test` runs locally but no GitHub Actions. Tests should gate merges.
