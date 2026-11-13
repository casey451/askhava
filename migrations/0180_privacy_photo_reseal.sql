-- privacy (T354): key rotation re-seals stored photos (PRIV-023, PRIV-024; D-2626, D-3542).
-- `rotate_photos` writes each photo sealed under an older VAULT_KEY version again under the active
-- version as a new object, then moves its privacy.photo row to that object and version in one
-- statement fenced on the old values, so the app role gets UPDATE on exactly those two columns;
-- id, job_id and stored_at stay fixed. A column grant only: nothing is rewritten or locked.

GRANT UPDATE (key_version, storage_key) ON privacy.photo TO askhava_app;
