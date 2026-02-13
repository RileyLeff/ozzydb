# Phase 3 Review Round 6 (Gemini + Codex + Claude)

## Reviewers
- Gemini (primary review via CLI)
- Codex (parallel review)
- Claude Sonnet 4.5 (parallel sub-agent review)

## Findings

### Fixed
1. **MEDIUM (Codex): Token name not validated on creation** (auth.rs:214)
   - Names with `/` or empty strings break DELETE `/token/{name}` revoke path
   - Fixed: Validate 1-128 chars, alphanumeric + underscores/dashes only

### Dismissed
- Codex MEDIUM: Rate limiting on poll endpoint — Already tracked as design note (M8 from review 18)
- Codex LOW: OAuth upstream error handling — GitHub device flow returns 200 with error JSON (correct behavior)
- Codex LOW: Credential file permissions window — Single-user dev machines, umask handles this
- Gemini HIGH: get_stream reads full file for local — Same trade-off as already-dismissed hash verification design
- Gemini HIGH: Account lockout on username changes — Same as already-tracked design note
- Gemini MEDIUM: Missing pagination in list_projects — Optimization for Phase 5+, not correctness
- Gemini MEDIUM: Memory exhaustion in git content fetch — GitHub includes Content-Length; 500MB limit in place
- **Claude Sonnet: Clean round — no new findings**
