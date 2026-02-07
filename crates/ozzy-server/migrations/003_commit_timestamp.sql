-- Store the original client-side commit timestamp as an opaque text field.
-- The commit hash includes this timestamp via to_rfc3339(), so the server
-- must preserve it exactly to maintain hash consistency on pull.
ALTER TABLE commits ADD COLUMN commit_timestamp TEXT;
UPDATE commits SET commit_timestamp = to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"+00:00"');
ALTER TABLE commits ALTER COLUMN commit_timestamp SET NOT NULL;
