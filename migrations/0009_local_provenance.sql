BEGIN;
-- Source-owned observation snapshot, NOT accepted entity external identities.
-- NULL means no durable extraction yet; do not invent totals from display fields.
-- Versioned typed JSON preserves repeated values and origins without an EAV schema.
ALTER TABLE file_metadata_observation ADD COLUMN provenance_json TEXT
    CHECK (provenance_json IS NULL OR json_valid(provenance_json));
PRAGMA user_version = 9;
COMMIT;
