# Review Series 1 Wrapup - Integration Test Plan & Results

Date: 2026-02-07
Summary of 21 review rounds across 3 models (Claude Opus 4.6, Codex gpt-5.3-codex, Gemini 3 Pro).

## Review Statistics

| Metric | Value |
|--------|-------|
| Total rounds | 21 |
| Total verified fixes | ~100 |
| Test count (final) | 172 (159 non-Docker + 12 Docker + 1 doc-test) |
| False positive rate (later rounds) | ~30% |

## Severity Trend

- **Rounds 2-5**: Foundational bugs (hash identity, path traversal, auth bypass)
- **Rounds 6-9**: Robustness (module injection, push races, atomicity)
- **Rounds 10-14**: Diminishing returns (input validation, TOML escaping)
- **Rounds 15-17**: Multi-model second wind (timestamp hash, transform canonicalization)
- **Rounds 18-21**: Defense-in-depth (safe slicing, symlink detection, bracket depth)

## Tests Added

### 1. Server Integration Tests (Docker/Postgres) — 7 new tests

| Test | Validates | Status |
|------|-----------|--------|
| `test_push_rejects_path_traversal_filenames` | R6, R8: Path sanitization | PASS |
| `test_push_without_lockfile_succeeds` | R15: Lockfile sentinel handling | PASS |
| `test_error_responses_use_consistent_format` | R21 C-L1: API error contract | PASS |
| `test_project_scoped_token_cannot_access_account_endpoints` | R21 C-H1: Auth scope enforcement | PASS |
| `test_push_with_tags` | R18-20: Tag push + pull by tag ref | PASS |
| `test_second_push_advances_ref` | R17: Ref update on successive pushes | PASS |
| `test_commit_hash_roundtrip` (pre-existing, fixed) | Hash integrity after push/pull | PASS |

### 2. Core Unit Tests — 17 new tests + 1 expanded

| Test | Validates | File |
|------|-----------|------|
| `test_canonicalize_json_string_escapes_keys_with_special_chars` | R15: JSON key escaping | canon.rs |
| `test_canonicalize_json_surrogate_pairs` | R9: Unicode surrogate pairs | canon.rs |
| `test_canonicalize_json_float_trimming_edge_cases` | R8: Float canonicalization | canon.rs |
| `test_canonicalize_json_u64_branch` | R13: Large u64 precision | canon.rs |
| `test_canonicalize_source_mixed_crlf_and_lf` | General: Mixed line endings | canon.rs |
| `test_hash_source_directory_skips_pycache` | R13: __pycache__ exclusion | canon.rs |
| `test_hash_source_directory_skips_hidden_files` | General: Hidden file exclusion | canon.rs |
| `test_hash_source_directory_cross_platform_separators` | R19 M4: Path normalization | canon.rs |
| `test_list_of_struct_roundtrip` | R19, R21: Nested type parsing | schema.rs |
| `test_struct_with_multiple_bracket_types` | R21 G-M7: Bracket depth tracking | schema.rs |
| `test_dict_with_list_value_roundtrip` | R19 M1: Dict with complex values | schema.rs |
| `test_timestamp_all_units_roundtrip` | General: Timestamp variants | schema.rs |
| `test_time32_time64_duration_roundtrip` | General: Temporal types | schema.rs |
| `test_large_list_roundtrip` | R19 M7: large_list type | schema.rs |
| `test_schema_diff_detects_changes` | General: Schema diff correctness | schema.rs |
| `test_validate_pipeline_missing_column` | General: Pipeline validation | schema.rs |
| `test_validate_safe_name_strict_ascii_pattern` (expanded) | R6: Name validation edge cases | project.rs |

### 3. CLI Integration Tests — 7 new tests

| Test | Validates | Status |
|------|-----------|--------|
| `test_status_shows_modified_vs_new_endpoints` | R19 H5: Status labeling | PASS |
| `test_commit_with_no_changes_shows_nothing_to_commit` | R7: Clean state detection | PASS |
| `test_multiple_transforms_same_file_distinct_hashes` | R15: Transform identity | PASS |
| `test_data_rm` | General: Data source removal | PASS |
| `test_transform_rm` | General: Transform removal | PASS |
| `test_tag_operations` | R18-20: Tag CRUD operations | PASS |

## Bugs Found & Fixed During Testing

### 1. Server: Transform composite hash not preserved in pull response (BUG FIX)

**File**: `crates/ozzy-server/src/api/v1/push_pull.rs`

The server's pull handler was setting `"hash": t.content_hash` for transforms, where `content_hash` is the raw source content hash. But `Transform.hash` should be the composite identity hash (`blake3(source_hash + function_name + lockfile_hash + runtime + params_schema_hash)`).

**Fix**: Recompute the composite hash from stored components during pull, and include `source_hash` in the response:
```rust
let composite_hash = transform_hash(&t.content_hash, &t.function_name, ...);
serde_json::json!({
    "hash": composite_hash,
    "source_hash": t.content_hash,
    ...
})
```

This also fixed the pre-existing `test_commit_hash_roundtrip` which was failing because the pulled commit couldn't reproduce its own hash.

### 2. Test: `build_commit` used source hash as composite hash (TEST FIX)

**File**: `crates/ozzy-server/tests/integration_tests.rs`

The test helper `build_commit()` was setting `Transform.hash = blake3(source)` instead of the correct composite hash. Fixed to use `compute_transform_hash()`.

### 3. Test: `test_transform_rm` used multi-transform file (TEST FIX)

**File**: `crates/ozzy-cli/tests/integration_test.rs`

The `transform rm` command refuses to delete from files containing multiple transforms. Test was using a multi-transform file. Fixed to use a single-transform file.

## Final Test Counts

| Crate | Unit | Integration | Docker Integration | Total |
|-------|------|-------------|-------------------|-------|
| ozzy-core | 58 | — | — | 58 |
| ozzy-cli | 16 | 33 | — | 49 |
| ozzy-server | 19 | 15 API + 9 DB + 8 storage | 12 | 63 |
| **Total** | **93** | **65** | **12** | **170** |
