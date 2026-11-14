-- Sprint 4: autenticação, sessões e pareamento seguro.
ALTER TABLE users ADD COLUMN must_change_password INTEGER NOT NULL DEFAULT 0 CHECK (must_change_password IN (0, 1));
ALTER TABLE users ADD COLUMN password_changed_at TEXT;

ALTER TABLE sessions ADD COLUMN csrf_hash TEXT;

ALTER TABLE api_tokens ADD COLUMN expires_at TEXT;
ALTER TABLE api_tokens ADD COLUMN extension_origin TEXT;

CREATE TABLE extension_pairing_challenges (
    id TEXT PRIMARY KEY NOT NULL,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    code_hash TEXT NOT NULL,
    display_code TEXT NOT NULL,
    token_name TEXT NOT NULL,
    extension_origin TEXT,
    expires_at TEXT NOT NULL,
    confirmed_at TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE INDEX idx_pairing_challenges_expiry ON extension_pairing_challenges(expires_at);
CREATE INDEX idx_api_tokens_expiry ON api_tokens(expires_at);
