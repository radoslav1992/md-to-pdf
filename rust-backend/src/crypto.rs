use pbkdf2::pbkdf2_hmac;
use rand::RngCore;
use sha2::Sha256;
use subtle::ConstantTimeEq;

use crate::error::ConvertError;

const PBKDF2_ITERATIONS: u32 = 100_000;
const SALT_LEN: usize = 16;
const HASH_LEN: usize = 32;
const TOKEN_LEN: usize = 32;

pub fn hash_password(password: &str) -> Result<(String, String), ConvertError> {
    let mut salt = [0u8; SALT_LEN];
    rand::thread_rng().fill_bytes(&mut salt);
    let hash = derive(password.as_bytes(), &salt);
    Ok((hex::encode(hash), hex::encode(salt)))
}

pub fn verify_password(password: &str, salt_hex: &str, hash_hex: &str) -> bool {
    let salt = match hex::decode(salt_hex) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let expected = match hex::decode(hash_hex) {
        Ok(h) => h,
        Err(_) => return false,
    };
    if expected.len() != HASH_LEN {
        return false;
    }
    let actual = derive(password.as_bytes(), &salt);
    actual.ct_eq(&expected).into()
}

pub fn random_token() -> String {
    let mut buf = [0u8; TOKEN_LEN];
    rand::thread_rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

fn derive(password: &[u8], salt: &[u8]) -> [u8; HASH_LEN] {
    let mut out = [0u8; HASH_LEN];
    pbkdf2_hmac::<Sha256>(password, salt, PBKDF2_ITERATIONS, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify_roundtrip() {
        let (hash, salt) = hash_password("hunter2").unwrap();
        assert!(verify_password("hunter2", &salt, &hash));
        assert!(!verify_password("wrong", &salt, &hash));
    }

    #[test]
    fn salts_are_unique() {
        let (h1, s1) = hash_password("same").unwrap();
        let (h2, s2) = hash_password("same").unwrap();
        assert_ne!(s1, s2);
        assert_ne!(h1, h2);
    }

    #[test]
    fn tokens_are_unique() {
        let a = random_token();
        let b = random_token();
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
    }
}
