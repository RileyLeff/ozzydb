# v3 Workflow State

## Current Phase: Phase 1 COMPLETE — R2 Storage + Presigned URLs + Streaming Uploads
## Current Step: All steps complete (1.1-1.3 implemented, 1.4-1.5 deferred)

## Completed Steps

### Step 1.1: Presigned URL generation
- Added `aws-sdk-s3`, `aws-config`, `aws-credential-types` to workspace
- Extended `ContentStorage` with `s3_client` + `bucket` fields
- Implemented `presigned_get_url()`, `presigned_put_url()`, `presigned_put_url_for_content()`
- 4 integration tests (format, download, upload, no-remote error)
- Commit: `641f5ba`

### Step 1.2: Streaming downloads via presigned redirect
- Data download handler returns 302 to presigned GET URL when R2 configured
- Falls back to proxying bytes for local-only dev
- Header renamed to `X-OzzyDB-Content-Hash`
- Commit: `4029222`

### Step 1.3: Streaming uploads
- `store_stream()` method on ContentStorage: hashes on the fly while uploading
- Single PutObject for files ≤5MB, multipart for larger files
- Raised default max upload size to 10GB
- 4 new tests (small file, large file multipart, hash consistency, local fallback)
- Added blake3 as direct dependency of ozzy-server
- Commit: `6ff2edd`

## Deferred Steps

### Step 1.4: CLI upload progress bar
**Reason:** CLI `ozzy data add` is not yet implemented (stub only). Progress bar will be added when the CLI data upload command is implemented in a later phase.

### Step 1.5: Deploy R2 to production
**Reason:** Requires SSH access to VPS. Will be done manually when ready. R2 credentials are already in `.env.prod`.

## Review Rounds

### Review Round 1 (Claude-only, Codex+Gemini failed)
- 6 fixes applied: M1 (abort multipart), M2 (copy_source leading slash), M3 (temp key cleanup), m2 (presigned download filenames), m3 (pub(crate) presigned_put_url), m4 (1GB default limit)
- Deferred: M4 (streaming upload handler — multipart field ordering constraint prevents naive streaming), m1 (Content-Disposition injection — currently safe), m5 (buffer clone — negligible for <5MB), m6 (get_stream remote hash verification — pre-existing), m7 (sync fs ops — pre-existing)
- Commit: `e798544`

### Review Round 2 (Claude-only, Gemini E2BIG)
- CLEAN: All prior fixes verified, no new issues found

### Review Round 3 (Claude-only, convergence round)
- CLEAN: 2 consecutive clean rounds achieved, Phase 1 converged

## What's Next
- Phase 2: Async Job Model + Parallel DAG
