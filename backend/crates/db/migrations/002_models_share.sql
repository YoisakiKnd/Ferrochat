CREATE INDEX IF NOT EXISTS chats_user_updated ON chats(user_id, updated_at);

ALTER TABLE provider_models ADD COLUMN params_json TEXT NOT NULL DEFAULT '{}';

CREATE TABLE IF NOT EXISTS provider_key_health (
    provider_id TEXT NOT NULL,
    key_fingerprint TEXT NOT NULL,
    last_error TEXT NOT NULL DEFAULT '',
    cooldown_until INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (provider_id, key_fingerprint)
);
