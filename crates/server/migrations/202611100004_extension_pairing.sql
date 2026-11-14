-- Suporte a escopos e rastreamento de uso em api_tokens
ALTER TABLE api_tokens ADD COLUMN scopes TEXT NOT NULL DEFAULT 'downloads:write';
ALTER TABLE api_tokens ADD COLUMN last_used_at TEXT;
ALTER TABLE api_tokens ADD COLUMN revoked_at TEXT;
