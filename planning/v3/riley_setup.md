# v3 Setup — Riley's Tasks

Things that require human action (account creation, key generation, etc.). Once done, put the values where indicated so the agent can wire them into the codebase.

---

## 1. Cloudflare R2 (do first — Fly depends on this)

### Steps

1. Log into [dash.cloudflare.com](https://dash.cloudflare.com)
2. ~~Copy your **Account ID**~~ — **Already have it**: 
3. Navigate to **Storage & databases** → **R2** → **Create bucket**
   - Bucket name: `ozzydb`
   - Location: Automatic (or hint Europe since Hetzner is in Germany)
   - Storage class: Standard
4. Go to **R2** → **Overview** → **Manage R2 API tokens** (top right) → **Create API token**
   - Token name: `ozzydb-server`
   - Permissions: **Object Read & Write**
   - Bucket scope: **Apply to specific buckets only** → select `ozzydb`
   - TTL: leave blank (no expiration)
   - Client IP filtering: optionally restrict to `46.225.111.110` (VPS IP)
5. **Copy the credentials immediately** — the Secret Access Key is shown once and cannot be retrieved again

### Where to put the values

Fill in the R2 section in `/.env.prod.example` (repo root), then copy to `.env.prod` on the VPS at `/opt/ozzydb/crates/ozzy-server/docker/.env.prod`.

Then tell the agent "R2 credentials are in `.env.prod`" and it will redeploy and verify.

### Pricing

| Resource | Free tier (monthly) | Paid rate |
|----------|-------------------|-----------|
| Storage | 10 GB | $0.015/GB |
| Class A ops (PUT, LIST, DELETE) | 1 million | $4.50/million |
| Class B ops (GET, HEAD) | 10 million | $0.36/million |
| **Egress** | **Unlimited** | **$0 (always free)** |

A small deployment will easily stay in the free tier.

### Notes

- Leave **public access disabled** — the server mediates all access
- Leave **CORS disabled** — not needed unless we add browser-direct uploads later
- R2 supports S3-compatible presigned URLs (AWS Sig V4) — needed for Fly machines to read/write data
- Presigned POST (multipart form) is **not supported** on R2, but PUT is fine

---

## 2. Fly.io (do second)

### Important: no GPU support

Fly.io **deprecated GPU machines in August 2025**. CPU-bound data transforms only. Modal is the future GPU option.

### Steps

1. Sign up at [fly.io/app/sign-up](https://fly.io/app/sign-up) (GitHub/Google/email). Credit card required.
2. Install flyctl:
   ```bash
   # macOS
   brew install flyctl
   ```
3. Log in:
   ```bash
   fly auth login
   ```
4. Create the compute app:
   ```bash
   fly apps create --machines --name ozzydb-compute
   ```
   This is just a namespace for machines — no Dockerfile needed.
5. Generate a **deploy token** (app-scoped, least privilege):
   ```bash
   fly tokens create deploy -a ozzydb-compute --name "ozzydb-server" --expiry 8760h
   ```
   Copy the token — it's shown once.
6. Authenticate Docker to Fly's registry (for pushing compute images):
   ```bash
   fly auth docker
   ```

### Where to put the values

Fill in the Fly.io section in `/.env.prod.example` (repo root), then copy to `.env.prod` on the VPS at `/opt/ozzydb/crates/ozzy-server/docker/.env.prod`.

`FLY_API_URL=https://api.machines.dev` is hardcoded as the default — no need to set it unless overriding.

Then tell the agent "Fly credentials are in `.env.prod`" and it will wire them into the server config and deploy.

### Token permissions

The deploy token can: create/start/stop/destroy machines, read status/events, manage volumes — all within `ozzydb-compute` only. It **cannot** access other apps, billing, or org settings.

### Image registry

Fly **cannot pull from private external registries** (GHCR private, Docker Hub private). The agent will push compute images to Fly's own registry:

```bash
docker tag ozzydb-runner:latest registry.fly.io/ozzydb-compute:latest
docker push registry.fly.io/ozzydb-compute:latest
```

Public images (e.g., `python:3.12-slim`) work directly — no registry push needed.

### Pricing

| Tier | CPUs | Memory | Cost/hour |
|------|------|--------|-----------|
| shared-cpu-1x | 1 shared | 256MB | $0.003 |
| shared-cpu-2x | 2 shared | 512MB | $0.006 |
| shared-cpu-4x | 4 shared | 1GB | $0.011 |
| performance-1x | 1 dedicated | 2GB | $0.045 |
| performance-2x | 2 dedicated | 4GB | $0.089 |
| performance-4x | 4 dedicated | 8GB | $0.179 |

A typical 30-second job on shared-cpu-2x costs ~$0.00005. 1,000 jobs/day ≈ $1.40/month.

Free trial: 2 hours runtime or 7 days, whichever first. No ongoing free tier.

### Key caveats

- **Cold start**: ~10-15 seconds per new machine (image pull + VM boot)
- **Machine limit**: Default 50 machines per app. Contact billing@fly.io to increase.
- **Failed machines**: Non-zero exit + `auto_destroy` delays destruction ~2 hours
- **Regions**: `fra` (Frankfurt) is closest to Hetzner. Fallbacks: `ams`, `lhr`, `iad`.

---

## Checklist

### R2
- [ ] Create Cloudflare account (or use existing)
- [ ] Copy Account ID
- [ ] Create `ozzydb` bucket
- [ ] Create API token (Object Read & Write, scoped to bucket)
- [ ] Copy Access Key ID + Secret Access Key
- [ ] Fill R2 values in `/.env.prod.example`, copy to VPS `.env.prod`
- [ ] Tell agent "R2 is ready"

### Fly.io
- [x] Install flyctl (`brew install flyctl`)
- [ ] Create Fly.io account + add credit card
- [ ] `fly auth login`
- [ ] `fly apps create --machines --name ozzydb-compute`
- [ ] `fly tokens create deploy -a ozzydb-compute --name "ozzydb-server" --expiry 8760h`
- [ ] Copy token
- [ ] `fly auth docker`
- [ ] Fill Fly values in `/.env.prod.example`, copy to VPS `.env.prod`
- [ ] Tell agent "Fly is ready"
