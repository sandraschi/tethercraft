//! R4 scam shield: banking, payment, crypto-wallet, password-manager, and
//! government-ID windows and sites are blurred for the remote viewer and
//! remote input into them is blocked while a stranger is connected.
//!
//! The decision engine here is real: given a window title, app identifier, or
//! URL it answers shielded-or-not. The OS sensors (browser URL from the
//! accessibility tree, frontmost-window tracking) are platform work and report
//! through [`ShieldObservation`]; the host never trusts the controller's word.

use serde::{Deserialize, Serialize};

/// Curated per-country lists. Austria first per PRD R6; Germany alongside
/// because Sandra supports family across the border.
pub fn default_bank_domains() -> Vec<String> {
    vec![
        "george.at".to_string(),
        "erste-bank.at".to_string(),
        "sparkasse.at".to_string(),
        "bawag.at".to_string(),
        "raiffeisen.at".to_string(),
        "bankaustria.at".to_string(),
        "unicredit".to_string(),
        "ing.at".to_string(),
        "ing.de".to_string(),
        "dkb.de".to_string(),
        "comdirect.de".to_string(),
        "paypal.com".to_string(),
        "bitpanda.com".to_string(),
        "coinbase.com".to_string(),
        "oenb.at".to_string(),
        "finanzonline.at".to_string(),
        "usp.gv.at".to_string(),
    ]
}

pub fn default_wallet_apps() -> Vec<String> {
    vec![
        "1password".to_string(),
        "bitwarden".to_string(),
        "keepass".to_string(),
        "lastpass".to_string(),
        "dashlane".to_string(),
    ]
}

/// Native banking and payment app identifiers (short names, not domains).
pub fn default_bank_apps() -> Vec<String> {
    vec![
        "george".to_string(),
        "erste bank".to_string(),
        "sparkasse".to_string(),
        "bawag".to_string(),
        "raiffeisen".to_string(),
        "bank austria".to_string(),
        "paypal".to_string(),
        "bitpanda".to_string(),
    ]
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShieldLists {
    pub bank_domains: Vec<String>,
    pub wallet_apps: Vec<String>,
    pub bank_apps: Vec<String>,
}

impl ShieldLists {
    pub fn defaults() -> Self {
        Self {
            bank_domains: default_bank_domains(),
            wallet_apps: default_wallet_apps(),
            bank_apps: default_bank_apps(),
        }
    }
}

/// What the OS sensor reports about the frontmost window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ShieldObservation {
    Url(String),
    App { id: String, title: String },
}

/// True when the remote viewer must get a blurred frame and no remote input.
pub fn shielded(lists: &ShieldLists, obs: &ShieldObservation) -> bool {
    match obs {
        ShieldObservation::Url(url) => {
            let lower = url.to_lowercase();
            lists.bank_domains.iter().any(|d| domain_hit(&lower, d))
        }
        ShieldObservation::App { id, title } => {
            let li = id.to_lowercase();
            let lt = title.to_lowercase();
            lists
                .wallet_apps
                .iter()
                .any(|a| li.contains(a) || lt.contains(a.as_str()))
                || lists
                    .bank_apps
                    .iter()
                    .any(|a| li.contains(a) || lt.contains(a.as_str()))
                || lists.bank_domains.iter().any(|d| lt.contains(d.as_str()))
        }
    }
}

fn domain_hit(url_lower: &str, domain: &str) -> bool {
    let d = domain.to_lowercase();
    // Match the registrable part: "https://login.george.at/x" contains "george.at".
    // Guard against "notgeorge.at" style false positives by requiring a boundary.
    match url_lower.find(d.as_str()) {
        Some(pos) => {
            if pos == 0 {
                return true;
            }
            let before = url_lower.as_bytes()[pos - 1];
            before == b'.' || before == b'/' || before == b':' || before == b'@'
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banks_shielded_others_not() {
        let lists = ShieldLists::defaults();
        assert!(shielded(
            &lists,
            &ShieldObservation::Url("https://login.george.at/session".to_string())
        ));
        assert!(shielded(
            &lists,
            &ShieldObservation::Url("https://www.paypal.com/signin".to_string())
        ));
        assert!(shielded(
            &lists,
            &ShieldObservation::Url("https://finanzonline.at/portal".to_string())
        ));
        assert!(!shielded(
            &lists,
            &ShieldObservation::Url("https://example.com/news".to_string())
        ));
        assert!(!shielded(
            &lists,
            &ShieldObservation::Url("https://notgeorge.at.fake.example/".to_string())
        ));
    }

    #[test]
    fn wallet_and_bank_apps_shielded() {
        let lists = ShieldLists::defaults();
        assert!(shielded(
            &lists,
            &ShieldObservation::App {
                id: "bitwarden-desktop".to_string(),
                title: "Bitwarden".to_string(),
            }
        ));
        assert!(shielded(
            &lists,
            &ShieldObservation::App {
                id: "george-app".to_string(),
                title: "George - Erste Bank".to_string(),
            }
        ));
        assert!(!shielded(
            &lists,
            &ShieldObservation::App {
                id: "notepad".to_string(),
                title: "Untitled".to_string(),
            }
        ));
    }
}
