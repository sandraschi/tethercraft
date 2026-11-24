//! L3 egui controller window (Tier 3 start).
//!
//! Peer list sidebar, session view fed by the capture/codec path, an
//! always-visible End session button while connected, Ctrl+wheel zoom, an
//! audit tail, and the R1 install gate: a non-skippable full question with a
//! 20-second wait before the answer buttons activate. Until the live run the
//! frames are synthetic (`fake_frame`); the window, gates, and zoom are real.

use chrono::{DateTime, Utc};
use eframe::egui;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// How long the R1 screen waits before the answer buttons activate.
pub const ONBOARDING_WAIT_SECS: u64 = 20;
/// Outgoing-only window after a yes/not-sure answer (matches core R1).
pub const OUTGOING_ONLY_SECS: i64 = 24 * 60 * 60;

pub struct ControllerUi {
    pub peers: Vec<(String, String)>,
    pub selected: Option<String>,
    pub zoom: f32,
    pub session_active: bool,
    pub recording: bool,
    pub outgoing_only: bool,
    pub onboarding_open: bool,
    pub onboarding_opened_at: Option<Instant>,
    pub marker_path: Option<PathBuf>,
    pub frame_no: u64,
    pub texture: Option<egui::TextureHandle>,
    pub decode_errors: u64,
    pub audit: Vec<String>,
}

pub fn clamp_zoom(z: f32) -> f32 {
    z.clamp(0.25, 4.0)
}

impl ControllerUi {
    pub fn new(
        peers: Vec<(String, String)>,
        outgoing_only: bool,
        needs_answer: bool,
        marker_path: Option<PathBuf>,
    ) -> Self {
        Self {
            peers,
            selected: None,
            zoom: 1.0,
            session_active: false,
            recording: false,
            outgoing_only,
            onboarding_open: needs_answer,
            onboarding_opened_at: needs_answer.then(Instant::now),
            marker_path,
            frame_no: 0,
            texture: None,
            decode_errors: 0,
            audit: Vec::new(),
        }
    }

    pub fn select(&mut self, id: &str) {
        self.selected = Some(id.to_string());
    }

    /// Controller sessions stay allowed in outgoing-only mode (outgoing-only
    /// blocks hosting/incoming, not the local person reaching out).
    pub fn start_session(&mut self) {
        if self.selected.is_none() || self.onboarding_open {
            return;
        }
        self.session_active = true;
        self.recording = true;
        self.push_audit("session started (local recording on)");
    }

    pub fn end_session(&mut self) {
        self.session_active = false;
        self.recording = false;
        self.frame_no = 0;
        self.texture = None;
        self.push_audit("session ended by local person");
    }

    pub fn push_audit(&mut self, line: &str) {
        self.audit.push(line.to_string());
        if self.audit.len() > 200 {
            let excess = self.audit.len() - 200;
            self.audit.drain(..excess);
        }
    }

    /// Answer the R1 question from the local UI. Persists the marker when a
    /// path was given. Returns the persisted marker for the caller to inspect.
    pub fn answer_onboarding(
        &mut self,
        yes_or_unsure: bool,
        now: DateTime<Utc>,
    ) -> OnboardingMarker {
        let marker = if yes_or_unsure {
            OnboardingMarker {
                answered: true,
                flagged: true,
                until: Some(now + chrono::Duration::seconds(OUTGOING_ONLY_SECS)),
            }
        } else {
            OnboardingMarker {
                answered: true,
                flagged: false,
                until: None,
            }
        };
        self.onboarding_open = false;
        self.outgoing_only = marker.active(now);
        if let Some(path) = &self.marker_path {
            let _ = std::fs::write(path, render_marker(&marker));
        }
        self.push_audit(if yes_or_unsure {
            "R1 answered yes/not-sure: outgoing-only for 24 h"
        } else {
            "R1 answered no: full mode"
        });
        marker
    }

    fn paint_frame(&mut self, ctx: &egui::Context) {
        let (meta, px) = tethercraft_capture::fake_frame(320, 200, self.frame_no);
        self.frame_no = self.frame_no.wrapping_add(1);
        match tethercraft_codec::to_egui_image(meta.width as usize, meta.height as usize, &px) {
            Ok(img) => match &mut self.texture {
                Some(h) => h.set(img, egui::TextureOptions::NEAREST),
                None => {
                    self.texture =
                        Some(ctx.load_texture("frame", img, egui::TextureOptions::NEAREST));
                }
            },
            Err(_) => {
                self.decode_errors = self.decode_errors.saturating_add(1);
            }
        }
    }
}

impl eframe::App for ControllerUi {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let dz = ctx.input(|i| i.zoom_delta());
        if dz != 1.0 {
            self.zoom = clamp_zoom(self.zoom * dz);
        }

        // R1 gate: modal, cannot be dismissed except by answering.
        if self.onboarding_open {
            let waited = self
                .onboarding_opened_at
                .map(|t| t.elapsed() >= Duration::from_secs(ONBOARDING_WAIT_SECS))
                .unwrap_or(false);
            egui::Window::new("Did someone tell you to install this?")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("Did someone call, email, text, or show a pop-up telling you to install TetherCraft?");
                    ui.label("Microsoft, Apple, banks, and the police never call you to install remote software.");
                    if !waited {
                        let left = ONBOARDING_WAIT_SECS.saturating_sub(
                            self.onboarding_opened_at
                                .map(|t| t.elapsed().as_secs())
                                .unwrap_or(0),
                        );
                        ui.label(format!("Read this first. Buttons activate in {left} s."));
                    }
                    ui.horizontal(|ui| {
                        let now = Utc::now();
                        if ui.add_enabled(waited, egui::Button::new("Yes / not sure")).clicked() {
                            self.answer_onboarding(true, now);
                        }
                        if ui.add_enabled(waited, egui::Button::new("No")).clicked() {
                            self.answer_onboarding(false, now);
                        }
                    });
                });
        }

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if self.session_active {
                    let end = egui::Button::new("End session").fill(egui::Color32::DARK_RED);
                    if ui.add(end).clicked() {
                        self.end_session();
                    }
                    ui.label(if self.recording { "REC on" } else { "REC off" });
                } else {
                    ui.label("No active session");
                }
                ui.label(format!("zoom {z:.2}x (Ctrl+wheel)", z = self.zoom));
            });
        });

        if self.outgoing_only {
            egui::TopBottomPanel::top("outgoing").show(ctx, |ui| {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    "Outgoing-only mode: incoming connections are disabled for 24 h. \
                     If someone is on the phone telling you what to do, hang up now.",
                );
            });
        }

        egui::SidePanel::left("peers").show(ctx, |ui| {
            ui.heading("Peers");
            let mut pick: Option<String> = None;
            for (id, alias) in &self.peers {
                let name = if alias.is_empty() {
                    id.clone()
                } else {
                    format!("{alias} ({id})")
                };
                if ui
                    .selectable_label(self.selected.as_deref() == Some(id.as_str()), name)
                    .clicked()
                {
                    pick = Some(id.clone());
                }
            }
            if let Some(id) = pick {
                self.select(&id);
            }
            if ui.button("Connect").clicked() {
                self.start_session();
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.session_active {
                self.paint_frame(ctx);
                if let Some(tex) = &self.texture {
                    let size = tex.size_vec2() * self.zoom;
                    ui.image((tex.id(), size));
                }
                if self.decode_errors > 0 {
                    ui.label(format!("decode errors: {}", self.decode_errors));
                }
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("Pick a peer and press Connect.");
                });
            }
        });

        egui::TopBottomPanel::bottom("audit").show(ctx, |ui| {
            ui.collapsing("Audit", |ui| {
                egui::ScrollArea::vertical()
                    .max_height(120.0)
                    .show(ui, |ui| {
                        for line in &self.audit {
                            ui.monospace(line);
                        }
                    });
            });
        });

        ctx.request_repaint();
    }
}

/// Persisted R1 answer. Tiny line format, no new dependencies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnboardingMarker {
    pub answered: bool,
    pub flagged: bool,
    pub until: Option<DateTime<Utc>>,
}

impl OnboardingMarker {
    pub fn active(&self, now: DateTime<Utc>) -> bool {
        match self.until {
            Some(u) => now < u,
            None => false,
        }
    }
}

pub fn render_marker(m: &OnboardingMarker) -> String {
    let until = match m.until {
        Some(u) => u.to_rfc3339(),
        None => "-".to_string(),
    };
    format!(
        "answered={a}\nflagged={f}\nuntil={until}\n",
        a = if m.answered { 1 } else { 0 },
        f = if m.flagged { 1 } else { 0 },
    )
}

pub fn parse_marker(text: &str) -> Option<OnboardingMarker> {
    let mut answered = false;
    let mut flagged = false;
    let mut until: Option<DateTime<Utc>> = None;
    for line in text.lines() {
        let (k, v) = line.split_once('=')?;
        match k.trim() {
            "answered" => answered = v.trim() == "1",
            "flagged" => flagged = v.trim() == "1",
            "until" => {
                if v.trim() != "-" {
                    match v.trim().parse::<DateTime<Utc>>() {
                        Ok(u) => until = Some(u),
                        Err(_) => return None,
                    }
                }
            }
            _ => return None,
        }
    }
    Some(OnboardingMarker {
        answered,
        flagged,
        until,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_clamps() {
        assert!(clamp_zoom(0.1) == 0.25);
        assert!(clamp_zoom(1.0) == 1.0);
        assert!(clamp_zoom(9.0) == 4.0);
    }

    #[test]
    fn select_start_end() {
        let mut ui = ControllerUi::new(vec![("p1".to_string(), String::new())], false, false, None);
        ui.start_session();
        assert!(!ui.session_active);
        ui.select("p1");
        ui.start_session();
        assert!(ui.session_active);
        assert!(ui.recording);
        ui.end_session();
        assert!(!ui.session_active);
        assert!(!ui.recording);
    }

    #[test]
    fn onboarding_blocks_session_start() {
        let mut ui = ControllerUi::new(vec![("p1".to_string(), String::new())], false, true, None);
        assert!(ui.onboarding_open);
        ui.select("p1");
        ui.start_session();
        assert!(!ui.session_active);
        ui.answer_onboarding(false, Utc::now());
        assert!(!ui.onboarding_open);
        ui.start_session();
        assert!(ui.session_active);
    }

    #[test]
    fn marker_roundtrip_and_expiry() {
        let now = DateTime::from_timestamp(1_800_000_000, 0).unwrap_or(DateTime::<Utc>::MIN_UTC);
        let m = OnboardingMarker {
            answered: true,
            flagged: true,
            until: Some(now + chrono::Duration::hours(1)),
        };
        let back = parse_marker(&render_marker(&m));
        assert!(back == Some(m.clone()));
        assert!(m.active(now));
        assert!(!m.active(now + chrono::Duration::hours(25)));
        let plain = OnboardingMarker {
            answered: true,
            flagged: false,
            until: None,
        };
        assert!(!plain.active(now));
        assert!(parse_marker("junk").is_none());
    }

    #[test]
    fn audit_bounded() {
        let mut ui = ControllerUi::new(Vec::new(), false, false, None);
        for _ in 0..250 {
            ui.push_audit("x");
        }
        assert!(ui.audit.len() == 200);
    }
}
