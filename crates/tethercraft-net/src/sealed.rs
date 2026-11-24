//! Sealed deployment configs (Tier 2).
//!
//! Password (Argon2id) encrypts with XChaCha20-Poly1305, then the operator's
//! Ed25519 key signs the sealed bytes. Unseal verifies the signature against
//! the pinned operator key first, so a tampered or foreign config never
//! reaches decryption, and never lowers local security settings.

use argon2::Argon2;
use chacha20poly1305::aead::{Aead, AeadCore, OsRng};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

use tethercraft_core::TetherError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealedBundle {
    pub version: u32,
    pub salt: Vec<u8>,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
    pub sig: Vec<u8>,
}

pub const SEALED_VERSION: u32 = 1;

fn derive_key(password: &[u8], salt: &[u8; 16]) -> Result<[u8; 32], TetherError> {
    if password.is_empty() || password.len() > 256 {
        return Err(TetherError::Invalid(
            "config password must be 1..=256 bytes".to_string(),
        ));
    }
    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(password, salt, &mut key)
        .map_err(|e| TetherError::Invalid(format!("argon2 failed: {e}")))?;
    Ok(key)
}

fn sign_bytes(signing: &SigningKey, salt: &[u8; 16], nonce: &[u8; 24], ct: &[u8]) -> [u8; 64] {
    let mut msg = Vec::with_capacity(salt.len() + nonce.len() + ct.len());
    msg.extend_from_slice(salt);
    msg.extend_from_slice(nonce);
    msg.extend_from_slice(ct);
    signing.sign(&msg).to_bytes()
}

/// Seal plaintext config bytes for storage or transport.
pub fn seal_config(
    plaintext: &[u8],
    password: &[u8],
    signing: &SigningKey,
) -> Result<SealedBundle, TetherError> {
    if plaintext.is_empty() || plaintext.len() > 1024 * 1024 {
        return Err(TetherError::Invalid(
            "config must be 1 byte..=1 MiB".to_string(),
        ));
    }
    let mut salt = [0u8; 16];
    getrandom::getrandom(&mut salt)
        .map_err(|e| TetherError::Invalid(format!("rng failed: {e}")))?;
    let key = derive_key(password, &salt)?;
    let cipher = XChaCha20Poly1305::new_from_slice(&key)
        .map_err(|e| TetherError::Invalid(format!("cipher setup: {e}")))?;
    let nonce_bytes = XChaCha20Poly1305::generate_nonce(&mut OsRng);
    let mut nonce = [0u8; 24];
    nonce.copy_from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(XNonce::from_slice(&nonce), plaintext)
        .map_err(|e| TetherError::Invalid(format!("encrypt failed: {e}")))?;
    let sig = sign_bytes(signing, &salt, &nonce, &ciphertext);
    Ok(SealedBundle {
        version: SEALED_VERSION,
        salt: salt.to_vec(),
        nonce: nonce.to_vec(),
        ciphertext,
        sig: sig.to_vec(),
    })
}

/// Unseal after verifying the operator signature against the pinned key.
pub fn unseal_config(
    bundle: &SealedBundle,
    password: &[u8],
    operator: &VerifyingKey,
) -> Result<Vec<u8>, TetherError> {
    if bundle.version != SEALED_VERSION {
        return Err(TetherError::Invalid(
            "unsupported sealed version".to_string(),
        ));
    }
    if bundle.salt.len() != 16 || bundle.nonce.len() != 24 || bundle.sig.len() != 64 {
        return Err(TetherError::Invalid("malformed sealed bundle".to_string()));
    }
    let mut salt = [0u8; 16];
    salt.copy_from_slice(&bundle.salt);
    let mut nonce = [0u8; 24];
    nonce.copy_from_slice(&bundle.nonce);
    let mut sig_bytes = [0u8; 64];
    sig_bytes.copy_from_slice(&bundle.sig);
    let mut msg = Vec::with_capacity(salt.len() + nonce.len() + bundle.ciphertext.len());
    msg.extend_from_slice(&salt);
    msg.extend_from_slice(&nonce);
    msg.extend_from_slice(&bundle.ciphertext);
    let sig = ed25519_dalek::Signature::from_bytes(&sig_bytes);
    operator.verify(&msg, &sig).map_err(|_| {
        TetherError::Denied("config signature not from pinned operator".to_string())
    })?;
    let key = derive_key(password, &salt)?;
    let cipher = XChaCha20Poly1305::new_from_slice(&key)
        .map_err(|e| TetherError::Invalid(format!("cipher setup: {e}")))?;
    cipher
        .decrypt(XNonce::from_slice(&nonce), bundle.ciphertext.as_ref())
        .map_err(|_| TetherError::Denied("config decrypt failed".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys() -> (SigningKey, VerifyingKey) {
        let sk = SigningKey::from_bytes(&[5u8; 32]);
        let vk = sk.verifying_key();
        (sk, vk)
    }

    #[test]
    fn seal_roundtrip() {
        let (sk, vk) = keys();
        let b = seal_config(b"{\"org\":\"home\"}", b"pw", &sk).unwrap_or(SealedBundle {
            version: 0,
            salt: Vec::new(),
            nonce: Vec::new(),
            ciphertext: Vec::new(),
            sig: Vec::new(),
        });
        assert!(b.version == SEALED_VERSION);
        let open = unseal_config(&b, b"pw", &vk).unwrap_or_default();
        assert!(open == b"{\"org\":\"home\"}");
    }

    #[test]
    fn wrong_password_tamper_and_signer_fail() {
        let (sk, vk) = keys();
        let b = seal_config(b"secret", b"pw", &sk).unwrap_or(SealedBundle {
            version: 0,
            salt: Vec::new(),
            nonce: Vec::new(),
            ciphertext: Vec::new(),
            sig: Vec::new(),
        });
        assert!(unseal_config(&b, b"other", &vk).is_err());
        let mut tampered = b.clone();
        if !tampered.ciphertext.is_empty() {
            tampered.ciphertext[0] ^= 0xFF;
        }
        assert!(unseal_config(&tampered, b"pw", &vk).is_err());
        let other_sk = SigningKey::from_bytes(&[9u8; 32]);
        let other_vk = other_sk.verifying_key();
        assert!(unseal_config(&b, b"pw", &other_vk).is_err());
        assert!(seal_config(b"", b"pw", &sk).is_err());
        assert!(seal_config(b"x", b"", &sk).is_err());
    }
}
