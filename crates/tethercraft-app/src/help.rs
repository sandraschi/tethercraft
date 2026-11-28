//! In-app operator guide: the Help window section. Topics are data
//! (`HelpTopic::ALL` plus `title` and `body`) so tests can check the guide
//! stays complete and ASCII-only; `render_help` draws it.

/// Help sub-tabs inside the Help window section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelpTopic {
    QuickStart,
    PeerBoard,
    TrustAndSafety,
    UseCases,
    Relay,
    ServiceAndCli,
    Troubleshooting,
}

impl HelpTopic {
    pub const ALL: [HelpTopic; 7] = [
        HelpTopic::QuickStart,
        HelpTopic::PeerBoard,
        HelpTopic::TrustAndSafety,
        HelpTopic::UseCases,
        HelpTopic::Relay,
        HelpTopic::ServiceAndCli,
        HelpTopic::Troubleshooting,
    ];

    pub fn title(self) -> &'static str {
        match self {
            HelpTopic::QuickStart => "Quick Start",
            HelpTopic::PeerBoard => "Peer Board",
            HelpTopic::TrustAndSafety => "Trust & Safety",
            HelpTopic::UseCases => "Use Cases",
            HelpTopic::Relay => "Relay (NAT)",
            HelpTopic::ServiceAndCli => "Service & CLI",
            HelpTopic::Troubleshooting => "Troubleshooting",
        }
    }

    pub fn body(self) -> &'static str {
        match self {
            HelpTopic::QuickStart => "TetherCraft is a pure-Rust remote desktop: native protocol, SPAKE2 password login, ChaCha20-Poly1305 E2E encryption, pinned host keys, host-enforced permissions.\n\nYour first 60 seconds:\n\n1. Host a peer: tethercraft host --port 21120 --password-file pw.txt --grants keyboard (add --fake-frames for synthetic frames, no GPU needed).\n\n2. Trust it: tethercraft book add goliath <host-id-hex> --addr 127.0.0.1:21120 --alias Goliath, then compare the fingerprint words the host prints with what you were told out of band.\n\n3. Connect: pick the card on the Board and press Connect to open the in-window dial dialog, or run tethercraft serve goliath --password-file pw.txt --frames 10 --out frame.ppm for a headless capture.\n\nPasswords never go on the command line: put them in a file and pass --password-file.",
            HelpTopic::PeerBoard => "The Board is the home view: one card per address-book peer, grouped into Ready to connect, First contact (verify words), and Needs an address.\n\nGreen dot: the host key is pinned, dialing enforces it. Amber dot: first contact, compare the host words out of band before trusting. Cards show alias, id, address, and trust tier.\n\nClick selects, double-click (or the Connect button) opens the in-window Connect modal with masked password, optional --via relay, and view-only toggle. First contact shows a top TOFU banner with the 8 fingerprint words until dismissed. Copy ID puts the peer id on the clipboard; the search box filters by name, id, tier, or address.",
            HelpTopic::TrustAndSafety => "Trust on first use: the first connect records the host key loudly; every later connect refuses a different key. Relay dials require a known key, never TOFU over the relay.\n\nR1 install gate: the first window run asks whether someone told you to install this; answering yes or not sure locks hosting into 24 h outgoing-only with a banner. R2 stranger tier: unknown peers get view-only plus denials on files, terminal, tunnel, blanking, and power, with a 30 min cap and local recording. Promotion to trusted_helper or managed cannot happen inside a live session.\n\nIf someone on the phone tells you what to click, hang up: Microsoft, Apple, banks, and the police never call you to install remote software.",
            HelpTopic::UseCases => "Supported: your own machines over LAN or Tailscale, safe family IT support with R1-R7 guardrails, scripted CLI and confirm-gated MCP automation, self-hosted E2E relay (--via), planned iOS/Android mobile controllers, and planned Tier 4/5 low-latency Game Streaming (UNDER DEVELOPMENT / TODO: zero-copy GPU encode, QUIC/UDP, Opus audio, relative mouse, gamepad passthrough).\n\nRestricted or intentionally NOT supported:\n\n- Onboarding-free QuickSupport for strangers: portable single-exe is supported, but R1 (20 s gate) and R2 (Stranger = view-only) are never skipped so phone scammers cannot grab instant control.\n\n- Stealth / hidden tray / silent consumer install: forbidden (hard non-goal) to prevent stalkerware and signed-RMM malware abuse. Active desktop sessions always show an indicator and audit log.\n\n- Screen blanking (Privacy Mode) for Strangers: blocked by R2 to stop bank-transfer scams; planned only for pre-enrolled Managed/TrustedHelper peers.\n\n- Public SaaS cloud relay: self-host tethercraft server or use Tailscale/LAN directly.",
            HelpTopic::Relay => "Direct LAN or Tailscale is the default. When the host sits behind NAT, relay through a self-hosted rendezvous server: host with tethercraft host --via SERVER (or --via on any controller verb or in the Connect modal), after one direct connect so the key is pinned.\n\nPairing uses one-time tokens: the host awaits, the controller bridges, both claim. All post-handshake traffic is ChaCha20-Poly1305 E2E encrypted (keys derived from SPAKE2 plus both Ed25519 keys with monotonic nonces), so the relay only pipes opaque ciphertext and cannot read or inject frames.",
            HelpTopic::ServiceAndCli => "Windows host service (needs elevation): tethercraft service install [--config PATH] writes a template config under %PROGRAMDATA%\\TetherCraft, then start, stop, and uninstall manage the SCM entry. service-run is hidden plumbing that hosts from the config file. The default template grants nothing until you edit it.\n\nCLI verbs mirror the GUI and MCP: files ls/get/put/mv/rm/mkdir, sync with --direction/--conflict plus --delete --confirm gates, exec, restart, shutdown, lock, tunnel local-port:remote-target, chat, screenshot, batch manifests, broadcast-file and collect-files across peers. Every verb supports --json; MCP tools need confirm=true and are audited.",
            HelpTopic::Troubleshooting => "Wrong password: the host rate-limits logins, wait and retry; check the password file for a trailing newline (trimmed automatically) and confirm the peer name.\n\nTampered key refused: the host reinstalled or a relay is feeding you a different key; verify the words and re-add with book add if the reinstall is legitimate.\n\nRelay wait never pairs: the host must be awaiting with the same server address, and the controller needs the pinned key (connect directly once first).\n\nService fails to start: install and start need elevation; the password_file path must be readable by LocalSystem; check the config JSON parses.\n\nEmpty board: add a peer first: tethercraft book add <peer> <host-key-hex> --addr host:port. No address on a card: re-add with --addr.",
        }
    }
}

/// Draw the Help section: topic tab row plus scrollable body text.
pub fn render_help(ui: &mut eframe::egui::Ui, active: &mut HelpTopic) {
    ui.heading("TetherCraft Operator Guide");
    ui.horizontal_wrapped(|ui| {
        for topic in HelpTopic::ALL {
            if ui
                .add(eframe::egui::Button::selectable(
                    *active == topic,
                    topic.title(),
                ))
                .clicked()
            {
                *active = topic;
            }
        }
    });
    ui.separator();
    eframe::egui::ScrollArea::vertical().show(ui, |ui| {
        ui.label(
            eframe::egui::RichText::new(active.title())
                .strong()
                .size(16.0),
        );
        ui.add_space(4.0);
        for para in active.body().split("\n\n") {
            ui.label(para);
            ui.add_space(6.0);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_topic_has_title_and_body() {
        assert!(HelpTopic::ALL.len() == 7);
        for topic in HelpTopic::ALL {
            assert!(!topic.title().is_empty());
            assert!(topic.body().len() > 100);
        }
    }

    #[test]
    fn help_stays_ascii() {
        for topic in HelpTopic::ALL {
            assert!(!topic.title().contains('—'), "em dash in {:?} title", topic);
            assert!(!topic.body().contains('—'), "em dash in {:?} body", topic);
            assert!(topic.body().is_ascii(), "non-ASCII in {:?} body", topic);
        }
    }

    #[test]
    fn help_covers_password_rule() {
        let bodies: Vec<&str> = HelpTopic::ALL.iter().map(|t| t.body()).collect();
        let all = bodies.join("\n");
        assert!(all.contains("--password-file"));
        assert!(all.contains("book add"));
    }
}
