# OzzyDB Security & Robustness Review (Round 6)

Date: February 7, 2026

Scope: Security-focused review of full codebase via dirgrab + `codex exec` (gpt-5.3-codex, xhigh reasoning). This round surfaced issues in input validation, crash resilience, and resource management — areas that Rounds 4 and 5 (functional correctness) did not cover.

## New Findings (Non-overlapping with Rounds 4–5)

### 1. Critical: Ref name path traversal in `resolve_ref` / `update_ref`

**Evidence:**
- `crates/ozzy-core/src/project.rs:432` — `resolve_ref` joins untrusted ref
  names directly into filesystem paths via `.join(ref_name.strip_prefix("refs/").unwrap_or(ref_name))`
- `crates/ozzy-core/src/project.rs:480` — `update_ref` does the same, then
  `create_dir_all` + `fs::write` — arbitrary file write relative to `.ozzy/refs/`
- `crates/ozzy-server/src/api/v1/push_pull.rs:435` — server accepts ref names
  from push payload; `normalize_ref_name` strips prefixes but does not validate
  the name itself (only tag names get `validate_safe_name` at line 473)
- `crates/ozzy-cli/src/commands/pull.rs:107` — CLI constructs local ref paths
  from remote-controlled names
- `crates/ozzy-cli/src/commands/commit.rs:68` — commit writes using
  `project.config.refs.head` from `ozzy.toml` without validation

**Impact:** Crafted ref names (e.g. `../../malicious`) can read/write arbitrary
files relative to the project root. Exploitable via malicious `ozzy.toml`,
crafted push payloads, or a compromised registry.

**Fix:** Add `validate_safe_name` (or a stricter ref-specific validator) at the
entry points of both `resolve_ref` and `update_ref`. Also validate branch ref
names server-side in the push handler (line 435), matching the existing tag
validation at line 473. Consider canonicalizing the resolved path and asserting
it stays within `.ozzy/refs/`.

### 2. High: Hash string slicing can panic server handlers (DoS)

**Evidence:**
- `crates/ozzy-server/src/storage/content.rs:84` — `&content_hash[0..2]`
- `crates/ozzy-server/src/storage/content.rs:93` — `&content_hash[2..4]`
- Push flow calls `storage.exists()` on hashes from the commit payload
  (`push_pull.rs:425`)

**Impact:** A malformed/short hash string in a push payload panics the request
handler. In a tokio server this kills the task but not the process — still a DoS
vector if repeated.

**Fix:** Validate hash format (length == 64, all hex chars) before any slicing.
Add a `validate_content_hash(hash: &str) -> Result<()>` helper and call it at
the storage layer entry points and/or at API boundary.

### 3. High: Memory exhaustion from buffering uploads/downloads

**Evidence:**
- `crates/ozzy-server/src/api/v1/push_pull.rs:237,252` — multipart fields fully
  materialized via `field.bytes().await` before size checks
- `crates/ozzy-server/src/api/v1/push_pull.rs:679,986` — pull/fetch responses
  built as `Vec<u8>` tar buffers in memory
- `crates/ozzy-core/src/registry/client.rs:432,501` — client buffers full tar
  responses into memory

**Impact:** Large datasets can OOM the server or client. A single large push can
take down the server process.

**Fix:** Defer to the server-side compute / streaming step (NEXT_STEPS step 2).
For uploads: stream multipart fields to disk/R2 with a size limit enforced
during streaming. For downloads: stream tar assembly directly to the response
body. For the client: stream tar extraction from the response body. This is a
larger refactor best done alongside the compute work.

### 4. Medium: Push persistence is not atomic

**Evidence:**
- `crates/ozzy-server/src/api/v1/push_pull.rs:446` — commit insertion
- `crates/ozzy-server/src/api/v1/push_pull.rs:462` — ref upsert
- `crates/ozzy-server/src/api/v1/push_pull.rs:497` — content ref registration

These are separate DB operations. A failure after commit insertion but before ref
upsert leaves an orphaned commit. A failure after ref upsert but before content
ref registration loses dedup tracking.

**Fix:** Wrap the commit-insert → ref-upsert → content-ref-registration sequence
in a single database transaction. sqlx supports this directly.

### 5. Medium: CLI panics on short hash strings in display output

**Evidence:**
- `crates/ozzy-cli/src/commands/pull.rs:207` — hash display truncation
- `crates/ozzy-cli/src/commands/cache.rs:24` — hash display truncation

Fixed-offset slicing for display purposes (`&hash[..8]`) panics if the stored
hash is corrupt or truncated.

**Fix:** Use `hash.get(..8).unwrap_or(hash)` or equivalent safe truncation
everywhere hashes are displayed.

### 6. Low: `validate_safe_name` allows whitespace and control characters

**Evidence:**
- `crates/ozzy-core/src/project.rs:19` — current validation blocks `/`, `\`,
  `..`, and leading `.`, but allows spaces, tabs, and other problematic
  characters in names used for filesystem paths and ref identifiers.

**Fix:** Restrict to `[a-zA-Z0-9_-]` or similar strict ASCII identifier pattern.

### 7. Low: Transform `file:function` parsing breaks on Windows drive paths

**Evidence:**
- `crates/ozzy-cli/src/commands/transform.rs:18` — `file.contains(':')` +
  `splitn(2, ':')` misparses `C:\path\transform.py:func`

**Fix:** Not urgent (OzzyDB targets Linux servers and macOS dev), but could be
addressed by splitting on the last `:` instead of the first, or requiring the
function delimiter to be `::`.

---

## Status of Prior Round Findings

### Round 4 — still open

| # | Finding | Status |
|---|---------|--------|
| 1 | Staged endpoint deletions bypassed by `run`/`dag` | **Partially addressed** — `run` now checks `.deleted` markers and bails, but `dag` still loads committed endpoints without checking deletion state |
| 2 | Multi-input schema validation effectively single-input | **Still open** — validation anchored to primary input |
| 5 | Python client ignores staged endpoint deletions | **Still open** — `Project.endpoints` includes committed endpoints even when staged for deletion |
| 6 | `transform add --name` unimplemented | **Still open** — CLI aborts with "not supported yet" |

### Round 4 — fixed by Round 5

| # | Finding | Fixed in |
|---|---------|----------|
| 3 | Pull doesn't reconcile staged endpoint state | `pull.rs:131` — `reconcile_staged_endpoints_after_pull()` |
| 4 | Decorator parser dict-style inputs | `commit.rs:314` — `extract_python_dict_raw()` |

### Round 5 — all fixed

All 9 findings from Round 5 were addressed by Codex in the same session,
including the Phase 2 tiered cache removal (NEXT_STEPS step 0).

---

## Recommended Fix Priority (Pre-Deploy)

1. **Ref path traversal** (Finding 1) — fix now, security critical
2. **Hash format validation** (Finding 2) — fix now, trivial guard
3. **Push atomicity** (Finding 4) — fix now, wrap in transaction
4. **Display hash truncation** (Finding 5) — fix now, one-liner each
5. **`validate_safe_name` strictness** (Finding 6) — fix now, small change
6. **Server branch ref validation** (part of Finding 1) — fix now
7. **Memory buffering** (Finding 3) — defer to streaming/compute step
8. **Windows path parsing** (Finding 7) — defer, low priority

## Validation

Review was read-only. No code changes made. Codex ran with `--sandbox read-only`.
Findings verified by manual inspection of referenced code locations.
