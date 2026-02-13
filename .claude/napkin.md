# Napkin

## Corrections
| Date | Source | What Went Wrong | What To Do Instead |
|------|--------|----------------|-------------------|
| 2026-02-07 | self | Tried `git add` without `cd` to repo root, got pathspec error | Always use absolute paths or ensure CWD is repo root for git commands |
| 2026-02-07 | self | Launched codex with `&` backgrounding which broke stdin pipe | Use the tool's `run_in_background` parameter instead of shell `&` |
| 2026-02-07 | self | Gemini `-p` flag fails with large prompts (exit code 13) | Pipe to gemini via `cat file | gemini > output` instead of `-p "$(cat)"` |
| 2026-02-07 | self | Used `replace_all: false` on pattern that matched multiple locations | Check uniqueness first; use `replace_all: true` or add more context to make pattern unique |
| 2026-02-07 | self | After extracting access.rs module, forgot to keep `ScopeAction` and `has_project_scope` in the original imports | When extracting shared code, check which symbols are still used directly vs through the new module |
| 2026-02-07 | user | Said "that's not convergence, that's me running out of usage" when both R11 reviews failed | Don't assume failure = convergence. Check if the issue is rate limits, context limits, or actual absence of bugs |
| 2026-02-13 | self | Codex `-o output.txt` flag doesn't produce file when run via Bash pipe | Use stdout redirect (`> output.txt`) instead of `-o` flag for Codex output |
| 2026-02-13 | self | `tokio::time::timeout` on `cmd.output()` doesn't kill child process | Use `cmd.spawn()` + named container + `docker kill` for Docker timeout handling |
| 2026-02-13 | self | `wait_with_output()` takes ownership, can't call `child.kill()` after timeout | Assign Docker container `--name` and use `docker kill {name}` on timeout instead |

## User Preferences
- Wants autonomous iteration: "i want you to run the process, not me!"
- Git commit after each round of review fixes
- Parallel multi-model reviews (codex/gemini/opus) with deduplication
- Reviews saved to `planning/reviews/codex_review_N.md`
- Run `cargo fmt --all && cargo test --workspace` + Python tests before committing
- Keep the review loop going until bugs are genuinely exhausted or usage runs out

## Patterns That Work
- Parallel tool launches for independent reviews (Gemini via Bash background, Opus via Task subagent)
- Deduplicating findings across models before fixing
- Fixing CRITICAL/HIGH first, then MEDIUM, then LOW
- Making all independent edits in parallel, then testing once
- Piping dirgrab output to gemini: `cat dirgrab.txt prompt.txt | gemini > output.txt`
- Growing the "already fixed" list in the review prompt to prevent re-reports

## Patterns That Don't Work
- Gemini with `-p` flag for large prompts (shell arg too large)
- Codex hitting OpenAI rate limits during heavy usage sessions
- Opus subagent reading entire dirgrab file (500KB+) uses most of its context on file reading, leaving little for analysis
- Assuming empty review output means "no bugs found" - could be tool/rate limit failure

## Deployment Notes

### Caddy 404 after frontend rebuild (RECURRING)
**Problem:** After rebuilding the frontend on the VPS (`npm run build`), ozzydb.com returns 404.
**Root cause:** The Caddy Docker container bind-mounts `/opt/ozzydb/frontend/build:/srv/frontend:ro`. Docker resolves bind mounts at container *creation* time. If the `build/` directory was empty or nonexistent when the container was first created, the mount points to a stale inode. Rebuilding creates a new `build/` directory but Caddy still sees the old (empty) mount.
**Fix:** After every frontend rebuild, restart Caddy:
```
cd /opt/ozzydb/crates/ozzy-server/docker && docker compose -f docker-compose.prod.yml --env-file .env.prod restart caddy
```
**Prevention:** Always run this restart after `npm run build` on the VPS. Consider adding a deploy script.

### VPS Access
- **Public IP:** 46.225.111.110
- **SSH:** `ssh root@46.225.111.110`
- **Tailscale:** `ssh root@ozzydb` (once authenticated, hostname is `ozzydb`)
- **Deploy frontend:** `cd /opt/ozzydb && git pull && cd frontend && npm run build && cd /opt/ozzydb/crates/ozzy-server/docker && docker compose -f docker-compose.prod.yml --env-file .env.prod restart caddy`
- **Deploy server:** `cd /opt/ozzydb && git pull && cd crates/ozzy-server/docker && docker compose -f docker-compose.prod.yml --env-file .env.prod build server && docker compose -f docker-compose.prod.yml --env-file .env.prod up -d`

## Domain Notes
- Cargo workspace: ozzy-core, ozzy-cli, ozzy-server
- 204 Rust tests (90 core + 110 server + 4 CLI unit) as of Phase 4 R18
- Python client at `clients/python/`, uses `uv` for package management
- `dirgrab` captures codebase context, respects `.dirgrabignore`
- Server uses Axum 0.8 with RPITIT (no async_trait)
- Database: PostgreSQL with sqlx (compile-time checked queries in dev, runtime in prod)
- Content storage: BLAKE3 hashing, local-first with optional R2 mirror
