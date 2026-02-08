# Opus Review 21 - Triage of Codex & Gemini External Reviews

Date: 2026-02-07
Sources: Codex Review 21 (gpt-5.3-codex xhigh), Gemini Review 21 (Gemini 3 Pro)

## Verified & Fixing

### HIGH
- **C-H1**: Auth scope bypass - project-scoped tokens can list/delete account-wide tokens
- **C-H3**: Pull accepts incomplete archives without failing (missing file check)
- **G-H3**: Symlink following during pull extraction (checked_destination)
- **G-H5**: Hash divergence after push/pull roundtrip (timestamp re-serialization)

### MEDIUM
- **C-M2**: Pull lockfile size check missing (DoS gap vs fetch_endpoint)
- **C-M3**: Temp-file naming race in concurrent async handlers (process::id not unique)
- **C-M5**: Invalid client input returns 500 instead of 400
- **C-M6**: Python client ref-path traversal via refs.head
- **G-M7**: Schema parsing bracket depth for timestamp[s, UTC] inside struct<>
- **G-M9**: Blocking FS operations in async server storage

### LOW
- **C-L1**: Error response schema inconsistent (auth returns {error:msg} vs protocol {error,message,details})

## Not Fixing (False Positives / Design Decisions)
- **G-H1**: Broken fetch CLI parsing (double @) - FALSE POSITIVE: code correctly strips @ at fetch.rs:51
- **G-H4**: Broken transform discovery (leading space) - FALSE POSITIVE: line 192 has no leading space
- **G-H2**: In-memory tar building - bounded by max_tar_size_bytes, acceptable
- **G-M6**: list_projects excludes collaborators - feature request, not bug
- **G-M8**: Multi-input schema validation - noted design limitation
- **G-M10**: Multiple decorators - edge case, single decorator per function is documented contract
- **G-M11**: Auth polling backoff - design note already added in R5
- **G-M12**: Multi-component branch names rejected - intentionally added in R5 M5
- **C-H2**: Pull writes extra archive files - lower priority, trust-the-server model
- **C-M1**: Fetch transform hash path mismatch - only for nested transforms (rare)
- **C-M4**: Schema hash trust - server already extracts schema independently
- **G-L13/L14/L15**: Performance optimizations - defer
