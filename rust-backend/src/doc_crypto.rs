//! AES-256-GCM document encryption keyed by a per-document password.
//!
//! Storage format: base64(`nonce || ciphertext || tag`) in the `content`
//! column. The salt used to derive the key is stored separately on the
//! document row so it can survive backups even if the content blob is
//! re-encoded.
//!
//! Threat model: the database alone is not enough to recover encrypted
//! content — an attacker must also know the document password. Passwords
//! are never sent anywhere; the server discards them after deriving the
//! key for one request.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use pbkdf2::pbkdf2_hmac;
use rand::RngCore;
use sha2::Sha256;

use crate::error::ConvertError;

const PBKDF2_ITERATIONS: u32 = 200_000;
const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;
const SALT_LEN: usize = 16;
/// Maximum allowed plaintext size. Mirrors the premium payload cap.
const MAX_PLAINTEXT: usize = 16 * 1024 * 1024;

pub struct Encrypted {
    pub salt_hex: String,
    /// base64(nonce || ciphertext)
    pub blob: String,
}

fn derive(password: &str, salt: &[u8]) -> [u8; KEY_LEN] {
    let mut key = [0u8; KEY_LEN];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), salt, PBKDF2_ITERATIONS, &mut key);
    key
}

pub fn validate_password(password: &str) -> Result<(), ConvertError> {
    if password.len() < 8 {
        return Err(ConvertError::BadRequest(
            "encryption password must be at least 8 characters".into(),
        ));
    }
    if password.len() > 1024 {
        return Err(ConvertError::BadRequest(
            "encryption password too long".into(),
        ));
    }
    Ok(())
}

pub fn encrypt(password: &str, plaintext: &str) -> Result<Encrypted, ConvertError> {
    validate_password(password)?;
    if plaintext.len() > MAX_PLAINTEXT {
        return Err(ConvertError::PayloadTooLarge(plaintext.len(), MAX_PLAINTEXT));
    }
    let mut salt = [0u8; SALT_LEN];
    rand::thread_rng().fill_bytes(&mut salt);
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);

    let key_bytes = derive(password, &salt);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key_bytes));
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| ConvertError::Internal(format!("encrypt: {e}")))?;

    let mut blob = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&ciphertext);
    Ok(Encrypted {
        salt_hex: hex::encode(salt),
        blob: STANDARD.encode(&blob),
    })
}

pub fn decrypt(password: &str, salt_hex: &str, blob_b64: &str) -> Result<String, ConvertError> {
    let salt = hex::decode(salt_hex).map_err(|_| {
        ConvertError::BadRequest("invalid encryption salt".into())
    })?;
    let blob = STANDARD.decode(blob_b64).map_err(|_| {
        ConvertError::BadRequest("invalid encrypted blob".into())
    })?;
    if blob.len() < NONCE_LEN + 16 {
        return Err(ConvertError::BadRequest(
            "encrypted blob is truncated".into(),
        ));
    }
    let (nonce_bytes, ciphertext) = blob.split_at(NONCE_LEN);
    let key_bytes = derive(password, &salt);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key_bytes));
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher.decrypt(nonce, ciphertext).map_err(|_| {
        ConvertError::Forbidden("wrong document password".into())
    })?;
    String::from_utf8(plaintext)
        .map_err(|_| ConvertError::Internal("decrypted bytes are not utf-8".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let enc = encrypt("hunter2!!", "secret document body").unwrap();
        let out = decrypt("hunter2!!", &enc.salt_hex, &enc.blob).unwrap();
        assert_eq!(out, "secret document body");
    }

    #[test]
    fn wrong_password_fails() {
        let enc = encrypt("hunter2!!", "secret").unwrap();
        let err = decrypt("wrong-pw!", &enc.salt_hex, &enc.blob).unwrap_err();
        assert!(matches!(err, ConvertError::Forbidden(_)));
    }

    #[test]
    fn rejects_short_password() {
        assert!(encrypt("short", "x").is_err());
    }

    #[test]
    fn each_encryption_uses_fresh_salt_and_nonce() {
        let a = encrypt("hunter2!!", "same").unwrap();
        let b = encrypt("hunter2!!", "same").unwrap();
        assert_ne!(a.salt_hex, b.salt_hex);
        assert_ne!(a.blob, b.blob);
    }

    #[test]
    fn truncated_blob_is_rejected() {
        let err = decrypt("hunter2!!", &hex::encode([0u8; 16]), "AA==").unwrap_err();
        assert!(matches!(err, ConvertError::BadRequest(_)));
    }
}
