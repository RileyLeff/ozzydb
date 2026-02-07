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

## Domain Notes
- Cargo workspace: ozzy-core, ozzy-cli, ozzy-server
- 108 Rust tests + 18 Python tests (as of R10)
- Python client at `clients/python/`, uses `uv` for package management
- `dirgrab` captures codebase context, respects `.dirgrabignore`
- Server uses Axum 0.8 with RPITIT (no async_trait)
- Database: PostgreSQL with sqlx (compile-time checked queries in dev, runtime in prod)
- Content storage: BLAKE3 hashing, local-first with optional R2 mirror
