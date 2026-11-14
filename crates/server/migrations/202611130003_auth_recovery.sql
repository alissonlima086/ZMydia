-- Código de recuperação offline: somente o hash é persistido.
ALTER TABLE users ADD COLUMN recovery_code_hash TEXT;
