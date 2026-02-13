# Phase 4 Review Round 20 — Claude Opus via Subagent

**Date:** 2026-02-13
**Model:** Claude Opus 4.6 (subagent — Codex still rate-limited)
**Commit:** `0d48554`
**Tests:** 90 core + 144 server + 44 CLI unit = 278 pass

## Findings (0 new bugs — CLEAN ROUND)

### Reported but already known:

1. **MEDIUM claimed — source_hash divergence server vs CLI** (known limitation #40)
   - Server uses `blake3(name:commit_sha)`, CLI uses `blake3(file_content)`
   - This is intentional: server can't access source file bytes (they're in git),
     so it uses commit SHA as a proxy. CLI and server are separate cache domains.
   - Documented in AGENT_WHITEBOARD round 4 (limitation #40).

2. **MEDIUM claimed — empty compute_inputs in fetch.rs** (known TODO)
   - `let compute_inputs: Vec<InputSpec> = Vec::new(); // TODO: resolve to local paths`
   - This is an explicitly marked incomplete feature, not a defect.
   - Documented in AGENT_WHITEBOARD: "compute_inputs resolution — incomplete feature."

## Status

Clean round 1 of 2. Need 1 more consecutive clean round for convergence.
