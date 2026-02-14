# Next Steps

## Immediate (do now)

### GitHub App setup
Configure the existing GitHub App for private repo access. Currently only public repos work — the server logs `GitHub App not configured — only public repos accessible`.

### End-to-end smoke test
Push a real project and fetch an endpoint on production. The compute pipeline (push → job creation → DAG orchestration → Docker/Fly execution → R2 upload → cache hit) has only been tested in E2E tests with MinIO, never against real R2.

### Automated pg_dump to R2
Cron job: `pg_dump | gzip | upload to R2`. No backup strategy exists today.

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
