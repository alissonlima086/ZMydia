use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
    sync::Arc,
};

use argon2::{Argon2, PasswordHasher, PasswordVerifier, password_hash::phc::PasswordHash};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::{
    aead::{AES_256_GCM, LessSafeKey, Nonce, UnboundKey},
    rand::{SecureRandom, SystemRandom},
};
use thiserror::Error;

const ENVELOPE_PREFIX: &str = "enc:v1:";
const NONCE_LEN: usize = 12;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("erro de E/S ao manipular chave mestra: {0}")]
    Io(#[from] std::io::Error),
    #[error("tamanho de chave mestra inválido (esperado 32 bytes, obtido {0})")]
    InvalidKeyLength(usize),
    #[error("falha na criptografia AEAD")]
    EncryptionFailed,
    #[error("falha na descriptografia AEAD (dados corrompidos ou chave incorreta)")]
    DecryptionFailed,
    #[error("falha ao decodificar base64 do envelope criptográfico: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("envelope criptográfico truncado ou inválido")]
    TruncatedEnvelope,
    #[error("segredo legado em texto plano recusado; execute a migração de startup")]
    PlaintextRejected,
    #[error("falha ao interpretar texto descriptografado como UTF-8: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
    #[error("falha ao processar hash de senha: {0}")]
    PasswordHash(String),
}

/// Gerenciador de operações criptográficas da aplicação
#[derive(Clone)]
pub struct AppCrypto {
    key: Arc<LessSafeKey>,
    rng: Arc<SystemRandom>,
}

impl AppCrypto {
    /// Cria uma instância a partir de 32 bytes de chave mestra
    pub fn from_key_bytes(key_bytes: &[u8; 32]) -> Result<Self, CryptoError> {
        let unbound =
            UnboundKey::new(&AES_256_GCM, key_bytes).map_err(|_| CryptoError::EncryptionFailed)?;
        Ok(Self {
            key: Arc::new(LessSafeKey::new(unbound)),
            rng: Arc::new(SystemRandom::new()),
        })
    }

    /// Carrega a chave mestra do caminho especificado ou gera uma nova de 256 bits com permissões 0600.
    pub fn init_or_generate(key_path: &Path) -> Result<Self, CryptoError> {
        if key_path.is_file() {
            let mut file = File::open(key_path)?;
            let mut key_bytes = Vec::new();
            file.read_to_end(&mut key_bytes)?;
            if key_bytes.len() != 32 {
                return Err(CryptoError::InvalidKeyLength(key_bytes.len()));
            }
            let mut key_array = [0u8; 32];
            key_array.copy_from_slice(&key_bytes);
            tracing::info!(path = %key_path.display(), "encryption master key loaded successfully");
            return Self::from_key_bytes(&key_array);
        }

        // Se não existir, gera 32 bytes de alta entropia
        let rng = SystemRandom::new();
        let mut key_array = [0u8; 32];
        rng.fill(&mut key_array)
            .map_err(|_| CryptoError::EncryptionFailed)?;

        if let Some(parent) = key_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // No Unix, cria com permissão 0600 (-rw-------)
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(key_path)?;
            file.write_all(&key_array)?;
        }

        #[cfg(not(unix))]
        {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(key_path)?;
            file.write_all(&key_array)?;
        }

        tracing::info!(path = %key_path.display(), "256-bit encryption master key generated and protected successfully");
        Self::from_key_bytes(&key_array)
    }

    /// Cifra uma string gerando o envelope 'enc:v1:<base64(nonce + ciphertext + tag)>'
    pub fn encrypt_string(&self, plaintext: &str) -> Result<String, CryptoError> {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        self.rng
            .fill(&mut nonce_bytes)
            .map_err(|_| CryptoError::EncryptionFailed)?;

        let nonce = Nonce::assume_unique_for_key(nonce_bytes);
        let mut in_out = plaintext.as_bytes().to_vec();

        self.key
            .seal_in_place_append_tag(nonce, ring::aead::Aad::empty(), &mut in_out)
            .map_err(|_| CryptoError::EncryptionFailed)?;

        // Payload serializado: Nonce (12B) + Ciphertext + Tag (16B)
        let mut combined = Vec::with_capacity(NONCE_LEN + in_out.len());
        combined.extend_from_slice(&nonce_bytes);
        combined.extend_from_slice(&in_out);

        let encoded = URL_SAFE_NO_PAD.encode(&combined);
        Ok(format!("{ENVELOPE_PREFIX}{encoded}"))
    }

    /// Descriptografa exclusivamente envelopes `enc:v1:`. O fallback legado em texto plano
    /// termina na migração de startup e não é aceito durante a operação normal.
    pub fn decrypt_string(&self, input: &str) -> Result<String, CryptoError> {
        let trimmed = input.trim();
        if !Self::is_encrypted(trimmed) {
            return Err(CryptoError::PlaintextRejected);
        }

        let base64_payload = &trimmed[ENVELOPE_PREFIX.len()..];
        let mut payload = URL_SAFE_NO_PAD.decode(base64_payload)?;

        if payload.len() < NONCE_LEN + AES_256_GCM.tag_len() {
            return Err(CryptoError::TruncatedEnvelope);
        }

        let mut nonce_bytes = [0u8; NONCE_LEN];
        nonce_bytes.copy_from_slice(&payload[..NONCE_LEN]);
        let nonce = Nonce::assume_unique_for_key(nonce_bytes);

        // Descriptografa o restante (ciphertext + tag) in-place
        let plaintext_slice = self
            .key
            .open_in_place(nonce, ring::aead::Aad::empty(), &mut payload[NONCE_LEN..])
            .map_err(|_| CryptoError::DecryptionFailed)?;

        let plaintext = String::from_utf8(plaintext_slice.to_vec())?;
        Ok(plaintext)
    }

    /// Verifica se a string está no formato de envelope criptográfico
    pub fn is_encrypted(val: &str) -> bool {
        val.trim().starts_with(ENVELOPE_PREFIX)
    }

    /// Gera um token de sessão seguro de 256 bits codificado em base64 URL-safe
    pub fn generate_session_token(&self) -> Result<String, CryptoError> {
        let mut token_bytes = [0u8; 32];
        self.rng
            .fill(&mut token_bytes)
            .map_err(|_| CryptoError::EncryptionFailed)?;
        Ok(URL_SAFE_NO_PAD.encode(token_bytes))
    }
}

/// Gera o hash de uma senha utilizando Argon2id com sal aleatório exclusivo
pub fn hash_password(password: &str) -> Result<String, CryptoError> {
    let argon2 = Argon2::default();
    argon2
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| CryptoError::PasswordHash(e.to_string()))
}

/// Verifica uma senha contra um hash PHC gerado por Argon2id
pub fn verify_password(password: &str, password_hash: &str) -> bool {
    let Ok(parsed_hash) = PasswordHash::new(password_hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_argon2id_hashing_and_verification() {
        let password = "correct-horse-47";
        let hash = hash_password(password).expect("hash_password deve funcionar");
        assert!(hash.starts_with("$argon2id$"));

        assert!(verify_password(password, &hash));
        assert!(!verify_password("senhaerrada", &hash));
        assert!(!verify_password("", &hash));
    }

    #[test]
    fn test_aead_encryption_and_decryption_roundtrip() {
        let crypto = AppCrypto::from_key_bytes(&[42u8; 32]).unwrap();
        let secret = "eba97867-bc34-4b0f-a774-1669a3643813";

        let encrypted = crypto.encrypt_string(secret).unwrap();
        assert!(AppCrypto::is_encrypted(&encrypted));
        assert!(encrypted.starts_with("enc:v1:"));
        assert_ne!(encrypted, secret);

        let decrypted = crypto.decrypt_string(&encrypted).unwrap();
        assert_eq!(decrypted, secret);
    }

    #[test]
    fn test_aead_unique_nonces_per_encryption() {
        let crypto = AppCrypto::from_key_bytes(&[7u8; 32]).unwrap();
        let secret = "minha_chave_secreta";

        let enc1 = crypto.encrypt_string(secret).unwrap();
        let enc2 = crypto.encrypt_string(secret).unwrap();

        assert_ne!(
            enc1, enc2,
            "Nonces devem ser aleatórios e gerar ciphertexts distintos"
        );
        assert_eq!(crypto.decrypt_string(&enc1).unwrap(), secret);
        assert_eq!(crypto.decrypt_string(&enc2).unwrap(), secret);
    }

    #[test]
    fn test_aead_tamper_detection() {
        let crypto = AppCrypto::from_key_bytes(&[9u8; 32]).unwrap();
        let secret = "dado_protegido";
        let mut encrypted = crypto.encrypt_string(secret).unwrap();

        // Modifica o último caractere
        let last_char = encrypted.pop().unwrap();
        let corrupted_char = if last_char == 'A' { 'B' } else { 'A' };
        encrypted.push(corrupted_char);

        let res = crypto.decrypt_string(&encrypted);
        assert!(
            res.is_err(),
            "Dados adulterados devem falhar com erro de autenticação AEAD"
        );
    }

    #[test]
    fn test_plaintext_is_rejected_after_finite_migration() {
        let crypto = AppCrypto::from_key_bytes(&[15u8; 32]).unwrap();
        let plain = "token_legado_sem_cifragem";

        assert!(!AppCrypto::is_encrypted(plain));
        assert!(matches!(
            crypto.decrypt_string(plain),
            Err(CryptoError::PlaintextRejected)
        ));
    }
}
