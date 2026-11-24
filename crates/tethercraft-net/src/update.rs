//! Signed auto-update check (Tier 4).
//!
//! Update metadata is verified against the pinned operator key before
//! anything is downloaded: signature first, then shape, then the no-downgrade
//! rule. Fetching the bytes is a later step; deciding is real today.

use ed25519_dalek::{Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use tethercraft_core::TetherError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedUpdate {
    pub version: String,
    pub url: String,
    pub sha256_hex: String,
    pub sig_hex: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateDecision {
    UpToDate,
    Available { version: String, url: String },
}

/// Parse `major.minor.patch` into numbers. Anything else is rejected.
pub fn parse_version(s: &str) -> Result<(u64, u64, u64), TetherError> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 3 {
        return Err(TetherError::Invalid(
            "version must be major.minor.patch".to_string(),
        ));
    }
    let mut out = [0u64; 3];
    for (i, p) in parts.iter().enumerate() {
        if p.is_empty() || p.len() > 10 {
            return Err(TetherError::Invalid("bad version part".to_string()));
        }
        out[i] = p
            .parse::<u64>()
            .map_err(|_| TetherError::Invalid("bad version part".to_string()))?;
    }
    Ok((out[0], out[1], out[2]))
}

fn canonical_bytes(version: &str, url: &str, sha: &str) -> Vec<u8> {
    let mut m = Vec::with_capacity(version.len() + url.len() + sha.len() + 2);
    m.extend_from_slice(version.as_bytes());
    m.push(b'\n');
    m.extend_from_slice(url.as_bytes());
    m.push(b'\n');
    m.extend_from_slice(sha.as_bytes());
    m
}

/// Sign update metadata with the operator key (used by the release process).
pub fn sign_update(
    signing: &ed25519_dalek::SigningKey,
    version: &str,
    url: &str,
    sha256_hex: &str,
) -> Result<SignedUpdate, TetherError> {
    use ed25519_dalek::Signer;
    parse_version(version)?;
    if url.trim().is_empty() || sha256_hex.len() != 64 {
        return Err(TetherError::Invalid("bad update metadata".to_string()));
    }
    let msg = canonical_bytes(version, url, sha256_hex);
    let sig = signing.sign(&msg);
    Ok(SignedUpdate {
        version: version.to_string(),
        url: url.to_string(),
        sha256_hex: sha256_hex.to_string(),
        sig_hex: hex::encode(sig.to_bytes()),
    })
}

/// Verify metadata and decide. Never downgrades, never trusts an unsigned blob.
pub fn verify_update(
    signed: &SignedUpdate,
    current: &str,
    operator: &VerifyingKey,
) -> Result<UpdateDecision, TetherError> {
    let new_v = parse_version(&signed.version)?;
    let cur_v = parse_version(current)?;
    if signed.url.trim().is_empty() || signed.sha256_hex.len() != 64 || signed.sig_hex.len() != 128
    {
        return Err(TetherError::Invalid("bad update metadata".to_string()));
    }
    let mut sig_bytes = [0u8; 64];
    hex::decode_to_slice(&signed.sig_hex, &mut sig_bytes)
        .map_err(|_| TetherError::Invalid("bad update signature".to_string()))?;
    let sig = ed25519_dalek::Signature::from_bytes(&sig_bytes);
    let msg = canonical_bytes(&signed.version, &signed.url, &signed.sha256_hex);
    operator
        .verify(&msg, &sig)
        .map_err(|_| TetherError::Denied("update not signed by pinned operator".to_string()))?;
    if new_v <= cur_v {
        Ok(UpdateDecision::UpToDate)
    } else {
        Ok(UpdateDecision::Available {
            version: signed.version.clone(),
            url: signed.url.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    fn keys() -> (SigningKey, VerifyingKey) {
        let sk = SigningKey::from_bytes(&[11u8; 32]);
        let vk = sk.verifying_key();
        (sk, vk)
    }

    const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn decided(s: &SignedUpdate, current: &str, vk: &VerifyingKey) -> UpdateDecision {
        verify_update(s, current, vk).unwrap_or(UpdateDecision::UpToDate)
    }

    #[test]
    fn versions_parse_strictly() {
        let v = parse_version("0.4.0").unwrap_or((9, 9, 9));
        assert!(v == (0, 4, 0));
        assert!(parse_version("1.2").is_err());
        assert!(parse_version("a.b.c").is_err());
        assert!(parse_version("").is_err());
        assert!(parse_version("1.2.3.4").is_err());
    }

    #[test]
    fn signed_update_flow() {
        let (sk, vk) = keys();
        let s = sign_update(&sk, "0.4.0", "https://example.invalid/t.exe", SHA).unwrap_or(
            SignedUpdate {
                version: String::new(),
                url: String::new(),
                sha256_hex: String::new(),
                sig_hex: String::new(),
            },
        );
        assert!(
            decided(&s, "0.3.0", &vk)
                == UpdateDecision::Available {
                    version: "0.4.0".to_string(),
                    url: "https://example.invalid/t.exe".to_string(),
                }
        );
        assert!(decided(&s, "0.4.0", &vk) == UpdateDecision::UpToDate);
        assert!(decided(&s, "9.9.9", &vk) == UpdateDecision::UpToDate);
        assert!(sign_update(&sk, "bogus", "u", SHA).is_err());
    }

    #[test]
    fn tamper_and_wrong_signer_fail() {
        let (sk, vk) = keys();
        let mut s = sign_update(&sk, "0.4.0", "https://example.invalid/t.exe", SHA).unwrap_or(
            SignedUpdate {
                version: String::new(),
                url: String::new(),
                sha256_hex: String::new(),
                sig_hex: String::new(),
            },
        );
        s.url = "https://evil.invalid/t.exe".to_string();
        assert!(verify_update(&s, "0.3.0", &vk).is_err());
        let other = SigningKey::from_bytes(&[12u8; 32]).verifying_key();
        let s2 = sign_update(&sk, "0.4.0", "https://example.invalid/t.exe", SHA).unwrap_or(
            SignedUpdate {
                version: String::new(),
                url: String::new(),
                sha256_hex: String::new(),
                sig_hex: String::new(),
            },
        );
        assert!(verify_update(&s2, "0.3.0", &other).is_err());
    }
}
