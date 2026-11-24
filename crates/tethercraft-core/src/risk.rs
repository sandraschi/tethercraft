//! R5 risk signals and R6 evidence bundles.
//!
//! The scorer is pure policy: given observed session signals it returns a
//! score, and above threshold the host freezes input and shows the local-only
//! interstitial. The sensor side (which window or URL is frontmost) is OS work
//! and lives behind the capture/input crates; the decision engine here is real.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// One observable fact about a session. Scored by [`score`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskSignal {
    YoungInstall,
    StrangerTier,
    BankUrlOpened,
    ShellOpened,
    ExecutableDownload,
    SecuritySettings,
    OwnSettingsOpened,
    BlankingRequested,
}

/// Score at or above which input freezes. See PRD R5.
pub const RISK_THRESHOLD: u8 = 50;

pub fn signal_weight(signal: RiskSignal) -> u8 {
    match signal {
        RiskSignal::YoungInstall => 20,
        RiskSignal::StrangerTier => 10,
        RiskSignal::BankUrlOpened => 30,
        RiskSignal::ShellOpened => 25,
        RiskSignal::ExecutableDownload => 20,
        RiskSignal::SecuritySettings => 25,
        RiskSignal::OwnSettingsOpened => 20,
        RiskSignal::BlankingRequested => 25,
    }
}

pub fn score(signals: &[RiskSignal]) -> u8 {
    let mut total: u16 = 0;
    for s in signals {
        total = total.saturating_add(u16::from(signal_weight(*s)));
    }
    total.min(100) as u8
}

/// Capture-excluded interstitial shown to the local person only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Interstitial {
    pub frozen: bool,
    pub reason: String,
}

pub fn evaluate(signals: &[RiskSignal]) -> Option<Interstitial> {
    let s = score(signals);
    if s >= RISK_THRESHOLD {
        Some(Interstitial {
            frozen: true,
            reason: format!(
                "risk score {s} at or above {t}: input frozen, local answer required",
                t = RISK_THRESHOLD
            ),
        })
    } else {
        None
    }
}

/// R6 evidence bundle: everything needed for a police report, no upload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceBundle {
    pub peer_id: String,
    pub peer_key_hex: String,
    pub ip: Option<String>,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub signals: Vec<String>,
    pub audit: Vec<String>,
}

impl EvidenceBundle {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weights_and_threshold() {
        assert!(signal_weight(RiskSignal::BankUrlOpened) == 30);
        let calm = [RiskSignal::StrangerTier];
        assert!(score(&calm) < RISK_THRESHOLD);
        assert!(evaluate(&calm).is_none());
        let scam = [
            RiskSignal::YoungInstall,
            RiskSignal::StrangerTier,
            RiskSignal::BankUrlOpened,
        ];
        assert!(score(&scam) >= RISK_THRESHOLD);
        let inter = evaluate(&scam);
        assert!(inter.is_some());
        if let Some(i) = inter {
            assert!(i.frozen);
        }
    }

    #[test]
    fn scammer_script_hits_every_signal() {
        // Scripted "scammer" controller: fresh install, stranger, opens bank,
        // opens a shell, downloads an exe, pokes settings, asks for blanking.
        let script = [
            RiskSignal::YoungInstall,
            RiskSignal::StrangerTier,
            RiskSignal::BankUrlOpened,
            RiskSignal::ShellOpened,
            RiskSignal::ExecutableDownload,
            RiskSignal::SecuritySettings,
            RiskSignal::OwnSettingsOpened,
            RiskSignal::BlankingRequested,
        ];
        assert!(score(&script) == 100.min(score(&script)));
        assert!(score(&script) >= RISK_THRESHOLD);
        assert!(evaluate(&script).is_some());
    }

    #[test]
    fn bundle_serialises() {
        let b = EvidenceBundle {
            peer_id: "scammer-1".to_string(),
            peer_key_hex: "ab12".to_string(),
            ip: Some("203.0.113.7".to_string()),
            started_at: DateTime::<Utc>::MIN_UTC,
            ended_at: DateTime::<Utc>::MIN_UTC,
            signals: vec!["BankUrlOpened".to_string()],
            audit: vec!["input denied".to_string()],
        };
        let j = b.to_json();
        assert!(j.contains("scammer-1"));
    }
}
