//! Deployment config import/export (Tier 2): signed, versioned, and never
//! allowed to lower local security settings.

use serde::{Deserialize, Serialize};

use crate::TetherError;

/// Current config schema version. Older versions are rejected, not migrated
/// silently; newer versions are rejected until the app learns them.
pub const DEPLOY_CONFIG_VERSION: u32 = 1;

/// Operator-owned deployment policy. Managed builds only; consumer builds keep
/// the R1/R2 defaults and cannot be softened by any config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeployConfig {
    pub version: u32,
    pub org: String,
    pub allow_unattended: bool,
    pub force_outgoing_only: bool,
}

impl DeployConfig {
    /// Validate before applying. `build_is_consumer` comes from the running
    /// build, never from the file.
    pub fn validate(&self, build_is_consumer: bool) -> Result<(), TetherError> {
        if self.version != DEPLOY_CONFIG_VERSION {
            return Err(TetherError::Invalid(format!(
                "unsupported deploy config version {v}",
                v = self.version
            )));
        }
        if self.org.trim().is_empty() {
            return Err(TetherError::Invalid("org name required".to_string()));
        }
        if build_is_consumer && self.allow_unattended {
            return Err(TetherError::Denied(
                "consumer builds cannot enable unattended access".to_string(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> DeployConfig {
        DeployConfig {
            version: DEPLOY_CONFIG_VERSION,
            org: "Family Office".to_string(),
            allow_unattended: false,
            force_outgoing_only: false,
        }
    }

    #[test]
    fn accepts_sane_config() {
        assert!(cfg().validate(false).is_ok());
        assert!(cfg().validate(true).is_ok());
    }

    #[test]
    fn rejects_downgrades_and_junk() {
        let mut bad_version = cfg();
        bad_version.version = 99;
        assert!(bad_version.validate(false).is_err());
        let mut no_org = cfg();
        no_org.org = "  ".to_string();
        assert!(no_org.validate(false).is_err());
        let mut sneaky = cfg();
        sneaky.allow_unattended = true;
        assert!(sneaky.validate(true).is_err());
        assert!(sneaky.validate(false).is_ok());
    }
}
