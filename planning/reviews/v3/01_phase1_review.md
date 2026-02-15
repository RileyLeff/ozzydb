Phase 1 (Steps 1.1-1.3) Code Review: R2 Presigned URLs, Streaming Downloads, Streaming Uploads
==================================================================================================

Files reviewed:
- /Users/rileyleff/Documents/dev/ozzydb/crates/ozzy-server/src/storage/content.rs
- /Users/rileyleff/Documents/dev/ozzydb/crates/ozzy-server/src/api/v1/data.rs
- /Users/rileyleff/Documents/dev/ozzydb/crates/ozzy-server/src/config.rs
- /Users/rileyleff/Documents/dev/ozzydb/crates/ozzy-server/tests/storage_tests.rs


MAJOR FINDINGS
==============

M1. [major] Multipart upload not aborted on error (content.rs:749-871, store_stream)

If any upload_part() call fails, or if the stream itself errors mid-upload, the
multipart upload is left in an incomplete state on R2/S3. Incomplete multipart
uploads consume storage and are never cleaned up. S3 charges for incomplete parts
until they expire or are explicitly aborted.

The code has no call to abort_multipart_upload() anywhere. If upload_part() fails
at line 796-805 or the final complete_multipart_upload() fails at line 840-848,
the upload_id is leaked.

Fix: Wrap the multipart upload body in a scope that calls
client.abort_multipart_upload() on any error path. A common pattern:

    let result = async {
        // upload parts...
        // complete...
    }.await;
    if result.is_err() {
        let _ = client.abort_multipart_upload()
            .bucket(bucket).key(&temp_key).upload_id(&upload_id)
            .send().await;
    }
    result?;


M2. [major] copy_source format may not work with R2 (content.rs:858)

The copy_source is formatted as:
    format!("{}/{}", bucket, temp_key)

The S3 CopyObject API requires the copy source in the format:
    /{bucket}/{key}    (with leading slash)
or using the x-amz-copy-source header with URL-encoded path.

Without the leading slash, this may work on some S3-compatible services but fail
on others. R2's documentation follows the S3 spec which requires the leading slash.
AWS SDK may handle this transparently, but it's worth verifying against actual R2.

Fix: Change to format!("/{}/{}", bucket, temp_key) or use the
.copy_source(format!("{}/{}", bucket, temp_key)) with explicit testing against R2.
The existing test_store_stream_large_file test (line 502) validates against MinIO,
not R2 itself, so this could pass tests but fail in production.


M3. [major] Temp key not cleaned up if copy_object fails (content.rs:855-869)

If the copy_object() call at line 855-861 fails, the content is stranded at the
temp key (_upload/{uuid}.tmp) and is never cleaned up. The function returns an
error but the temp key persists in R2 forever.

Fix: If copy_object fails, attempt to delete the temp key before returning the
error:

    match client.copy_object()...send().await {
        Ok(_) => {
            let _ = client.delete_object()...key(&temp_key)...send().await;
        }
        Err(e) => {
            let _ = client.delete_object()...key(&temp_key)...send().await;
            return Err(e).context("Failed to copy temp upload to content-addressed key");
        }
    }


M4. [major] Upload handler still buffers entire file in memory (data.rs:184-214)

The upload_data handler at data.rs:184 reads the entire file body into memory
with `field.bytes().await?` at line 204. Despite the v3 plan for Step 1.3 stating
"Replace memory-buffered upload with streaming," the handler itself was never
converted to use store_stream(). The store_stream() method exists in content.rs
but is unreachable from any API handler.

With max_upload_size_bytes raised to 10GB (config.rs:151), a single upload can
OOM the server since the entire file is buffered as bytes::Bytes before being
passed to storage.store().

This appears to be an incomplete implementation -- store_stream() was built but
never wired into the upload endpoint. The upload handler needs to be rewritten to
pass the multipart field body as a stream directly to store_stream().


MINOR FINDINGS
==============

m1. [minor] Content-Disposition header not sanitized against injection (data.rs:508)

The Content-Disposition header is built with:
    format!("attachment; filename=\"{}\"", filename)

The filename comes from atom.name (validated as [a-zA-Z0-9_-]+) concatenated with
a content-type-derived extension. Since atom.name is validated and the extension
comes from a fixed mapping, this is currently safe. However, if the name validation
ever changes to allow quotes or newlines, this becomes a header injection vector.

Consider using percent-encoding or the RFC 5987 filename* parameter for defense
in depth.


m2. [minor] Content-Disposition header on 302 redirect is ignored by browsers (data.rs:506-509)

When the server returns a 302, the browser follows the redirect to R2. The
Content-Disposition header on the 302 response itself is not forwarded to the
client by the browser -- the R2 response headers take precedence. Since R2 serves
the file without Content-Disposition (unless configured on the object metadata),
the download filename will be the R2 key (a hash), not the user-friendly name.

Fix options:
1. Set Content-Disposition on the R2 object during upload (via metadata/response-content-disposition)
2. Use the presigned URL's response-content-disposition override parameter:
   client.get_object()
       .bucket(bucket)
       .key(&key)
       .response_content_disposition(format!("attachment; filename=\"{}\"", filename))
       .presigned(presigning)
3. Document this as a known limitation


m3. [minor] presigned_put_url does not validate storage_key (content.rs:647)

The presigned_put_url() method accepts an arbitrary storage_key string without
validation. While presigned_put_url_for_content() validates through storage_key()
-> validate_content_hash(), the raw presigned_put_url() method could be called
with arbitrary keys including path traversal attempts (e.g., "../other-prefix/...").

This is currently only called from presigned_put_url_for_content() and tests, but
as a public method it should validate the key or be made private/pub(crate).


m4. [minor] 10GB default may be excessive for typical deployment (config.rs:151)

The max_upload_size_bytes default was raised from 100MB to 10GB (10737418240).
Since the upload handler still buffers entirely in memory (see M4), this creates
a denial-of-service vector: a single slow upload of ~10GB would consume all
available memory on the CX22 (4GB RAM). Even with streaming, 10GB is a large
default.

Consider a more conservative default (e.g., 1GB) until streaming uploads are
fully wired up, or at minimum until M4 is fixed.


m5. [minor] Small file path in store_stream clones buffer unnecessarily (content.rs:739)

At line 739, the small-file PutObject body uses `buffer.clone().into()`. Since
buffer is not used after this point (only cache_local_best_effort reads it, which
takes &[u8]), you could use `Bytes::from(buffer)` directly for the send, then use
the Bytes reference for caching. But since it's only for files <5MB, the
performance impact is negligible.


m6. [minor] get_stream does not hash-verify remote content (content.rs:488-493)

The get_stream() method verifies the content hash for local files (line 470-479)
but does NOT verify it for remote streams (line 488-493). It returns
`result.into_stream()` directly. The non-streaming `get()` method correctly
verifies the hash for both local and remote reads.

For content-addressed storage this means a corrupted R2 object would be served
without detection. This is a streaming-vs-correctness tradeoff, but should be
documented or addressed.


m7. [minor] Synchronous fs operations in async context (content.rs:401-402, 467-468, 511-513, 580-581)

Several methods use synchronous std::fs operations (std::fs::read, std::fs::metadata,
std::fs::remove_file) inside async functions. While most are for small metadata
operations, std::fs::read at line 402 and 467 reads the entire file synchronously,
which blocks the tokio runtime thread. This was partially addressed in a previous
review (G-M9) for store/hydrate but remains in get() and get_stream().


NOTES / OBSERVATIONS
====================

N1. [note] presigned_get_url TTL is hardcoded to 1 hour (data.rs:500)

The presigned URL TTL is hardcoded to 3600 seconds. This is reasonable for
interactive downloads but should be configurable if presigned URLs are ever used
for long-running compute jobs or shared links.


N2. [note] store_stream not used anywhere yet

store_stream() is a well-implemented method with good test coverage, but it has
zero callers in production code. The upload endpoint (data.rs:upload_data) still
buffers entirely. The presigned_put_url methods are also unused outside tests.
This is infrastructure built ahead of need, which is fine, but means the actual
streaming upload path is untested in integration with the API layer.


N3. [note] R2 multipart minimum part size

S3 requires minimum 5MB per part (except the last part). The PART_SIZE constant
at content.rs:691 is exactly 5MB, which meets this requirement. However, parts
can be slightly larger than 5MB due to chunk accumulation in part_buf before the
>= check (line 794). This is fine -- S3 allows parts up to 5GB.


N4. [note] Test coverage gaps

Tests that exist are solid, but there are gaps:
- No test for store_stream with an erroring stream (mid-upload failure)
- No test for store_stream with an empty stream (0 bytes)
- No test for concurrent store_stream calls
- No test for the 302 redirect path in download_data (requires API-level test)
- No test for presigned_put_url with invalid/malicious storage keys
- No test verifying that the temp key (_upload/*.tmp) is cleaned up after
  successful store_stream (this could be checked with a list operation)
- The test_presigned_url_without_remote_fails test has a dead `config` variable
  (line 421) that it drops later (line 465) for no reason


N5. [note] Local-only mode fallback is clean

The fallback to local-only storage when R2 is not configured is consistently
implemented across all new methods (store_stream buffers and delegates to store(),
presigned URLs return errors, download proxies bytes). This is good for local
dev ergonomics.


N6. [note] Bucket unwrap safety (content.rs:628, 651, 707)

Several methods do `self.bucket.as_ref().unwrap()` after checking s3_client is
Some. Since bucket and s3_client are always set together in the constructors, this
is safe. But a #[cfg(debug_assertions)] assert or an explicit error would be
more defensive than unwrap().


SUMMARY
=======

The Phase 1 implementation builds solid infrastructure for presigned URLs and
streaming uploads. The presigned URL generation (Step 1.1) and download redirect
(Step 1.2) are well-implemented. The store_stream method (Step 1.3) has correct
multipart logic and good test coverage against MinIO.

The critical gap is that store_stream is never wired into the upload API handler
(M4), meaning uploads still buffer entirely in memory. Combined with the 10GB
default limit (m4), this creates a practical DoS vector. The multipart upload
error handling also needs cleanup: abort on failure (M1) and temp key cleanup on
copy failure (M3). The copy_source format (M2) should be verified against actual
R2 before deployment.

The 302 redirect for downloads is correct but the Content-Disposition header
won't reach the client (m2), which degrades UX for browser-initiated downloads.
