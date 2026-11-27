//! Device keys (Ed25519), fingerprint words, SPAKE2 PAKE, pinned-key transport config.

pub mod rpc;
pub mod sealed;
pub mod session;
pub mod update;

use ed25519_dalek::{SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tethercraft_core::TetherError;

/// Generate a fresh device signing key from OS randomness.
pub fn generate_device_key() -> Result<SigningKey, TetherError> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes)
        .map_err(|e| TetherError::Invalid(format!("rng failed: {e}")))?;
    Ok(SigningKey::from_bytes(&bytes))
}

pub fn public_key_bytes(signing: &SigningKey) -> [u8; 32] {
    signing.verifying_key().to_bytes()
}

pub fn parse_public_key(bytes: &[u8]) -> Result<VerifyingKey, TetherError> {
    if bytes.len() != 32 {
        return Err(TetherError::Invalid("pubkey must be 32 bytes".to_string()));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(bytes);
    VerifyingKey::from_bytes(&arr).map_err(|e| TetherError::Invalid(format!("bad pubkey: {e}")))
}

/// Fingerprint of a public key: hex of SHA-256 plus 8 words for read-out.
pub fn fingerprint_hex(pubkey: &[u8; 32]) -> String {
    let mut h = Sha256::new();
    h.update(pubkey);
    hex::encode(h.finalize())
}

const WORDS: &[&str] = &[
    "alder", "amber", "anchor", "antler", "apricot", "archer", "arctic", "ash", "aspen", "atlas",
    "autumn", "badger", "birch", "bison", "blade", "bluff", "otter", "owl", "oxide", "paddle",
    "panda", "paper", "parrot", "pebble", "cedar", "cherry", "cinder", "clover", "comet", "copper",
    "coral", "cougar", "crane", "creek", "crow", "crystal", "cypress", "dahlia", "dawn", "delta",
    "denim", "dune", "eagle", "ember", "engine", "falcon", "fern", "field", "finch", "fjord",
    "flint", "forest", "forge", "fox", "frost", "garnet", "glacier", "grove", "gull", "harbor",
    "hawk", "hazel", "heron", "hickory", "hollow", "honey", "horizon", "ibis", "igloo", "indigo",
    "inlet", "iris", "island", "ivory", "jaguar", "jasper", "juniper", "kelp", "kestrel", "lake",
    "larch", "laurel", "lichen", "lilac", "linen", "lotus", "lumen", "lynx", "magnet", "maple",
    "marble", "marsh", "meadow", "mercury", "mesa", "meteor", "mica", "moss", "mountain", "mouse",
    "nimbus", "north", "nova", "oak", "onyx", "opal", "orchid", "oriole", "osprey", "peach",
    "pearl", "pepper", "pine", "pioneer", "plaza", "poplar", "prairie", "quartz", "quill", "radar",
    "raven", "reef", "ridge", "river", "robin", "rocket", "rose", "sable", "salmon", "sand",
    "sapphire", "sedge", "sequoia", "shadow", "signal", "silver", "slate", "sparrow", "spirit",
    "spruce", "storm", "summit", "sunset", "swift", "sycamore", "tamarack", "tango", "teal",
    "thrush", "thunder", "topaz", "trail", "trout", "tundra", "turtle", "umber", "vault", "velvet",
    "violet", "viper", "Vista", "walnut", "willow", "wolf", "wren", "yarrow", "juniper2", "zebra",
    "zephyr", "acorn", "basalt", "beacon", "brook", "butte", "canyon", "cliff", "cobalt", "condor",
    "coyote", "dove", "elm", "garlic", "gin", "herb", "ice", "jasmine", "kiwi", "lemon", "mango",
    "nectar", "olive", "onion", "palm", "plum", "quince", "sage", "thyme", "tomato", "tulip",
    "vega", "wheat", "yam", "yonder", "zinc", "alpaca", "bamboo", "cactus", "dingo", "echidna",
    "fennel", "gecko", "hyena", "iguana", "jackal", "koala", "lemur", "manatee", "narwhal",
    "ocelot", "pangolin", "quokka", "raccoon", "saola", "tapir", "uakari", "vicuna", "wombat",
    "xerus", "yak", "zebu", "aloe", "brine", "chalk", "drift", "estuary", "flora", "granit",
    "heath", "invar", "jetty", "knoll", "lagoon", "mural", "notch", "oasis", "pinnac", "quarry",
    "runnel", "shoal", "terrace", "upland", "vale", "wharf", "basin", "cirque", "dell", "esker",
    "firth", "glen", "hamlet", "inlet2", "kettle", "loch", "moor", "nunatak", "outwash", "polder",
    "reef2", "skerry", "tarn", "vale2", "wold",
];

/// Eight words derived from the fingerprint hash. Same key always gives same words.
pub fn fingerprint_words(pubkey: &[u8; 32]) -> Vec<String> {
    let mut h = Sha256::new();
    h.update(pubkey);
    let digest = h.finalize();
    digest
        .iter()
        .take(8)
        .map(|b| WORDS[*b as usize % WORDS.len()].to_string())
        .collect()
}

/// Minimal SPAKE2 handshake wrapper (Ed25519 group via spake2 crate).
/// Returns the two messages and checks both sides agree iff passwords match.
pub struct PakeHandshake {
    password: Vec<u8>,
}

impl PakeHandshake {
    pub fn new(password: &[u8]) -> Result<Self, TetherError> {
        if password.is_empty() || password.len() > 256 {
            return Err(TetherError::Invalid(
                "password must be 1..=256 bytes".to_string(),
            ));
        }
        Ok(Self {
            password: password.to_vec(),
        })
    }

    /// Run both sides locally (used in tests and LAN pairing). Real network
    /// sends msg_a over `PakeA` and msg_b over `PakeB` envelopes.
    pub fn agree_same_password(&self, other_password: &[u8]) -> Result<bool, TetherError> {
        use spake2::{Ed25519Group, Identity, Password, Spake2};
        let pw_a = Password::new(&self.password);
        let pw_b = Password::new(other_password);
        let id_a = Identity::new(b"controller");
        let id_b = Identity::new(b"host");
        let (s1, m1) = Spake2::<Ed25519Group>::start_a(&pw_a, &id_a, &id_b);
        let (s2, m2) = Spake2::<Ed25519Group>::start_b(&pw_b, &id_a, &id_b);
        let k1 = s1
            .finish(&m2)
            .map_err(|e| TetherError::Invalid(format!("pake a finish: {e:?}")))?;
        let k2 = s2
            .finish(&m1)
            .map_err(|e| TetherError::Invalid(format!("pake b finish: {e:?}")))?;
        Ok(k1 == k2)
    }
}

/// Pinned-key QUIC transport intent. Live handshake lands in Tier 1 verification;
/// this struct already enforces the "exact key or nothing" rule.
#[derive(Debug, Clone)]
pub struct PinnedTransport {
    pub expected_peer_key: [u8; 32],
}

impl PinnedTransport {
    pub fn new(expected_peer_key: [u8; 32]) -> Self {
        Self { expected_peer_key }
    }

    pub fn verify_peer(&self, presented: &[u8; 32]) -> Result<(), TetherError> {
        if presented != &self.expected_peer_key {
            return Err(TetherError::Denied(
                "peer key does not match pinned key".to_string(),
            ));
        }
        Ok(())
    }

    /// QUIC endpoint wiring is live-transport work; until the two-machine run
    /// it returns NotImplemented instead of a fake connected socket.
    pub fn connect(&self) -> Result<(), TetherError> {
        Err(TetherError::NotImplemented(
            "quic connect needs live two-machine run (Tier 1 exit)".to_string(),
        ))
    }
}

/// Host-side login rate limiter: N failures inside window locks the peer.
pub struct LoginRateLimiter {
    window: Duration,
    max_failures: u32,
    failures: HashMap<String, Vec<Instant>>,
}

impl LoginRateLimiter {
    pub fn new(max_failures: u32, window_secs: u64) -> Self {
        Self {
            window: Duration::from_secs(window_secs),
            max_failures,
            failures: HashMap::new(),
        }
    }

    pub fn record_failure(&mut self, peer: &str) {
        let now = Instant::now();
        let entry = self.failures.entry(peer.to_string()).or_default();
        entry.push(now);
        let window = self.window;
        entry.retain(|t| now.duration_since(*t) <= window);
    }

    pub fn locked(&self, peer: &str) -> bool {
        match self.failures.get(peer) {
            Some(v) => v.len() as u32 >= self.max_failures,
            None => false,
        }
    }

    pub fn record_success(&mut self, peer: &str) {
        self.failures.remove(peer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keygen_and_fingerprint_stable() {
        let k = generate_device_key().unwrap_or_else(|_| SigningKey::from_bytes(&[9u8; 32]));
        let pk = public_key_bytes(&k);
        assert!(fingerprint_hex(&pk).len() == 64);
        let w1 = fingerprint_words(&pk);
        let w2 = fingerprint_words(&pk);
        assert!(w1 == w2);
        assert!(w1.len() == 8);
    }

    #[test]
    fn pubkey_parse_rejects_short() {
        assert!(parse_public_key(&[0u8; 16]).is_err());
        assert!(parse_public_key(&[0u8; 32]).is_ok());
    }

    #[test]
    fn pake_same_password_agrees() {
        let h = PakeHandshake::new(b"correct horse").unwrap_or_else(|_| {
            PakeHandshake::new(b"fallback").unwrap_or_else(|_| {
                // new() only fails on empty/too-long input; both literals are valid.
                // Return path never taken; keeps deny-lints clean without unwrap.
                PakeHandshake {
                    password: b"fallback".to_vec(),
                }
            })
        });
        assert!(h.agree_same_password(b"correct horse").unwrap_or(false));
        assert!(!h.agree_same_password(b"wrong").unwrap_or(true));
    }

    #[test]
    fn pinned_transport_rejects_unknown_key() {
        let t = PinnedTransport::new([1u8; 32]);
        assert!(t.verify_peer(&[1u8; 32]).is_ok());
        assert!(t.verify_peer(&[2u8; 32]).is_err());
        assert!(t.connect().is_err());
    }

    #[test]
    fn rate_limiter_locks() {
        let mut r = LoginRateLimiter::new(3, 60);
        assert!(!r.locked("p"));
        r.record_failure("p");
        r.record_failure("p");
        assert!(!r.locked("p"));
        r.record_failure("p");
        assert!(r.locked("p"));
        r.record_success("p");
        assert!(!r.locked("p"));
    }
}
