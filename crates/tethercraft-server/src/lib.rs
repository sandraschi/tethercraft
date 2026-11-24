//! Self-hosted rendezvous and relay (Tier 3). Placeholder in Tier 1.

use tethercraft_core::TetherError;

pub fn run_server(_relay: bool) -> Result<(), TetherError> {
    Err(TetherError::NotImplemented(
        "rendezvous/relay server ships in Tier 3".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_is_tier3() {
        assert!(run_server(false).is_err());
    }
}
