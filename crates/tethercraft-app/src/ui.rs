//! L3 egui controller window.
//!
//! Window sections (Board, Session, Help) switch the central panel; the
//! Board is the RustDesk-style peer kanban, Session shows the live stream,
//! Help renders the operator guide. Includes the R1 install gate: a
//! non-skippable full question with a 20-second wait before the answer
//! buttons activate.

use chrono::{DateTime, Utc};
use eframe::egui;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tethercraft_input::InputEvent;

pub use super::board::{BoardCol, BoardState, PeerCard};
use super::help::{render_help, HelpTopic};

/// How long the R1 screen waits before the answer buttons activate.
pub const ONBOARDING_WAIT_SECS: u64 = 20;
/// Outgoing-only window after a yes/not-sure answer (matches core R1).
pub const OUTGOING_ONLY_SECS: i64 = 24 * 60 * 60;

/// Window sections. The board is the RustDesk-style home; Session shows the
/// live stream; Help renders the operator guide inside the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppSection {
    Board,
    Session,
    Help,
}

impl AppSection {
    pub const ALL: [AppSection; 3] = [AppSection::Board, AppSection::Session, AppSection::Help];

    pub fn title(self) -> &'static str {
        match self {
            AppSection::Board => "Board",
            AppSection::Session => "Session",
            AppSection::Help => "Help",
        }
    }
}

/// First-contact TOFU warning banner shown until the operator dismisses it.
#[derive(Debug, Clone)]
pub struct TofuNotice {
    pub peer_id: String,
    pub words: String,
    pub key_hex: String,
}

type DialResult = Result<(LiveSource, [u8; 32], String), String>;
pub type TofuCallback = Box<dyn Fn(&str, &[u8; 32])>;

struct DialLaunch {
    addr: String,
    password: String,
    view_only: bool,
    expected: Option<[u8; 32]>,
    via: Option<String>,
}

/// In-window connection dialog state (password prompt + background handshake).
pub struct DialPrompt {
    pub peer_id: String,
    pub display_name: String,
    pub addr: String,
    pub expected_key: Option<[u8; 32]>,
    pub password: String,
    pub view_only: bool,
    pub via: String,
    pub in_flight: Option<mpsc::Receiver<DialResult>>,
    pub error: Option<String>,
}

pub struct ControllerUi {
    pub board: BoardState,
    pub selected: Option<String>,
    pub section: AppSection,
    pub help_topic: HelpTopic,
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
    pub source: FrameSource,
    pub last_rect: Option<egui::Rect>,
    pub last_remote: Option<(u32, u32)>,
    pub last_sent_pos: Option<(i32, i32)>,
    pub live_dead: bool,
    pub pinned_keys: HashMap<String, [u8; 32]>,
    pub dial_prompt: Option<DialPrompt>,
    pub tofu_notice: Option<TofuNotice>,
    pub on_tofu: Option<TofuCallback>,
}

/// Where session frames come from. Synthetic keeps headless CI green; Live
/// carries the remote desktop once the handshake succeeds.
pub enum FrameSource {
    Synthetic,
    Live(LiveSource),
}

pub struct LiveSource {
    pub frames: mpsc::Receiver<(u32, u32, Vec<u8>)>,
    pub input: mpsc::Sender<InputEvent>,
    pub view_only: bool,
    pub greeting: String,
}

pub fn clamp_zoom(z: f32) -> f32 {
    z.clamp(0.25, 4.0)
}

/// Dial the host, run the PAKE + E2E handshake, and hand the encrypted session
/// to two threads: one pumps frames in, one drains local input out.
pub fn connect_live_pinned(
    addr: &str,
    password: &[u8],
    view_only: bool,
    expected_host_key: Option<[u8; 32]>,
    via: Option<&str>,
) -> Result<(LiveSource, [u8; 32], String), tethercraft_core::TetherError> {
    use std::sync::mpsc;
    use tethercraft_net::session::{self, SessionStream};
    use tethercraft_proto::{Envelope, MessageKind};

    let signing = tethercraft_net::generate_device_key()?;
    let stream = match via {
        Some(server) => {
            let expected = expected_host_key.ok_or_else(|| {
                tethercraft_core::TetherError::Invalid(
                    "relay needs a known host key: connect directly first (or use --pin-key)"
                        .to_string(),
                )
            })?;
            session::relay_connect(server, "tethercraft-ctrl", &hex::encode(expected))?
        }
        None => session::dial(addr, std::time::Duration::from_secs(10))?,
    };
    let mut sess = SessionStream::new(stream)?;
    let auth = session::controller_handshake(
        &mut sess,
        "controller",
        &signing,
        password,
        expected_host_key,
    )?;
    sess.set_frame_timeouts()?;
    let mut out_sess = sess.clone_session()?;
    let my_pub = tethercraft_net::public_key_bytes(&signing);

    let (frame_tx, frame_rx) = mpsc::channel::<(u32, u32, Vec<u8>)>();
    let (input_tx, input_rx) = mpsc::channel::<InputEvent>();
    let words = auth.host_words.join(" ");

    std::thread::spawn(move || {
        let mut decoder = match session::StreamDecoder::new() {
            Ok(d) => d,
            Err(_) => return,
        };
        while let Ok(env) = sess.recv_envelope() {
            match env.kind {
                MessageKind::Frame | MessageKind::FrameH264 => {
                    if let Ok(Some((w, h, rgba))) = decoder.decode(env.kind, &env.payload) {
                        if frame_tx.send((w, h, rgba)).is_err() {
                            break;
                        }
                    }
                }
                MessageKind::SessionEnd => break,
                _ => {}
            }
        }
    });

    std::thread::spawn(move || {
        let mut seq = 0u64;
        while let Ok(ev) = input_rx.recv() {
            let payload = match session::json_bytes(&ev) {
                Ok(p) => p,
                Err(_) => continue,
            };
            seq = seq.wrapping_add(1);
            let kind = match ev {
                InputEvent::TouchTap { .. } => MessageKind::TouchTap,
                InputEvent::TouchHold { .. } => MessageKind::TouchHold,
                InputEvent::TouchSwipe { .. } => MessageKind::TouchSwipe,
                _ => MessageKind::InputEvent,
            };
            if out_sess
                .send_envelope(&Envelope::new(kind, my_pub, payload))
                .is_err()
            {
                break;
            }
        }
        let _ = seq;
    });

    let live = LiveSource {
        frames: frame_rx,
        input: input_tx,
        view_only,
        greeting: format!("connected to {} (E2E, words: {words})", auth.host_id),
    };
    let key = auth.host_key;
    Ok((live, key, words))
}

impl ControllerUi {
    pub fn new(
        peers: Vec<PeerCard>,
        outgoing_only: bool,
        needs_answer: bool,
        marker_path: Option<PathBuf>,
    ) -> Self {
        Self {
            board: BoardState::new(peers),
            selected: None,
            section: AppSection::Board,
            help_topic: HelpTopic::QuickStart,
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
            source: FrameSource::Synthetic,
            last_rect: None,
            last_remote: None,
            last_sent_pos: None,
            live_dead: false,
            pinned_keys: HashMap::new(),
            dial_prompt: None,
            tofu_notice: None,
            on_tofu: None,
        }
    }

    pub fn select(&mut self, id: &str) {
        self.selected = Some(id.to_string());
    }

    /// Open the in-window dial + password dialog for a peer that has an address.
    /// Respects the R1 onboarding gate and refuses peers without an address.
    pub fn request_connect(&mut self, peer_id: &str) {
        if self.onboarding_open {
            return;
        }
        self.select(peer_id);
        let card = self.board.cards.iter().find(|c| c.id == peer_id).cloned();
        match card {
            Some(c) => match c.addr.as_deref().map(str::trim).filter(|a| !a.is_empty()) {
                Some(addr) => {
                    let expected_key = self.pinned_keys.get(&c.id).copied();
                    self.dial_prompt = Some(DialPrompt {
                        peer_id: c.id.clone(),
                        display_name: c.display_name(),
                        addr: addr.to_string(),
                        expected_key,
                        password: String::new(),
                        view_only: false,
                        via: String::new(),
                        in_flight: None,
                        error: None,
                    });
                }
                None => {
                    self.push_audit(&format!(
                        "cannot dial {}: no address yet (run tethercraft book add {} --addr host:port)",
                        c.id, c.id
                    ));
                }
            },
            None => {
                self.push_audit(&format!("cannot dial unknown peer {peer_id}"));
            }
        }
    }

    /// Poll a background handshake spawned by the in-window dial modal.
    fn poll_dial_result(&mut self, ctx: &egui::Context) {
        let outcome = match self.dial_prompt.as_mut().and_then(|p| p.in_flight.as_ref()) {
            Some(rx) => match rx.try_recv() {
                Ok(res) => Some(res),
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint();
                    None
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("dial thread exited unexpectedly".to_string()))
                }
            },
            None => None,
        };
        if let Some(res) = outcome {
            match res {
                Ok((live, key, words)) => {
                    if let Some(prompt) = self.dial_prompt.take() {
                        if prompt.expected_key.is_none() {
                            if let Some(cb) = &self.on_tofu {
                                cb(&prompt.peer_id, &key);
                            }
                            self.pinned_keys.insert(prompt.peer_id.clone(), key);
                            if let Some(card) =
                                self.board.cards.iter_mut().find(|c| c.id == prompt.peer_id)
                            {
                                card.has_key = true;
                            }
                            self.tofu_notice = Some(TofuNotice {
                                peer_id: prompt.peer_id.clone(),
                                words: words.clone(),
                                key_hex: hex::encode(key),
                            });
                            self.push_audit(&format!(
                                "TOFU recorded new host key for {} (verify words: {})",
                                prompt.peer_id, words
                            ));
                        }
                        self.set_live(live);
                    }
                }
                Err(msg) => {
                    if let Some(prompt) = self.dial_prompt.as_mut() {
                        prompt.in_flight = None;
                        prompt.error = Some(msg.clone());
                    }
                    self.push_audit(&format!("dial failed: {msg}"));
                }
            }
        }
    }

    fn render_dial_modal(&mut self, ctx: &egui::Context) {
        let mut close_modal = false;
        let mut launch: Option<DialLaunch> = None;

        if let Some(prompt) = self.dial_prompt.as_mut() {
            let busy = prompt.in_flight.is_some();
            egui::Window::new(format!("Connect to {}", prompt.display_name))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.label(format!("Address: {}", prompt.addr));
                    if prompt.expected_key.is_some() {
                        ui.colored_label(
                            egui::Color32::from_rgb(0, 140, 60),
                            "Host key pinned (mismatch will be refused).",
                        );
                    } else {
                        ui.colored_label(
                            egui::Color32::from_rgb(180, 120, 0),
                            "First contact (TOFU): compare the 8 words out of band after connect.",
                        );
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("Password:");
                        let resp = ui.add_enabled(
                            !busy,
                            egui::TextEdit::singleline(&mut prompt.password).password(true),
                        );
                        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            launch = Some(DialLaunch {
                                addr: prompt.addr.clone(),
                                password: std::mem::take(&mut prompt.password),
                                view_only: prompt.view_only,
                                expected: prompt.expected_key,
                                via: (!prompt.via.trim().is_empty())
                                    .then(|| prompt.via.trim().to_string()),
                            });
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Relay (--via, optional):");
                        ui.add_enabled(!busy, egui::TextEdit::singleline(&mut prompt.via));
                    });
                    ui.add_enabled(
                        !busy,
                        egui::Checkbox::new(&mut prompt.view_only, "View-only"),
                    );
                    if let Some(err) = &prompt.error {
                        ui.colored_label(egui::Color32::RED, err);
                    }
                    if busy {
                        ui.label("Connecting (SPAKE2 + E2E)...");
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(!busy, egui::Button::new("Connect"))
                            .clicked()
                        {
                            launch = Some(DialLaunch {
                                addr: prompt.addr.clone(),
                                password: std::mem::take(&mut prompt.password),
                                view_only: prompt.view_only,
                                expected: prompt.expected_key,
                                via: (!prompt.via.trim().is_empty())
                                    .then(|| prompt.via.trim().to_string()),
                            });
                        }
                        if ui.button("Cancel").clicked() {
                            close_modal = true;
                        }
                    });
                });

            if let Some(req) = launch {
                if req.password.is_empty() {
                    prompt.error = Some("password required".to_string());
                } else {
                    prompt.error = None;
                    let (tx, rx) = mpsc::channel();
                    prompt.in_flight = Some(rx);
                    std::thread::spawn(move || {
                        let res = connect_live_pinned(
                            &req.addr,
                            req.password.as_bytes(),
                            req.view_only,
                            req.expected,
                            req.via.as_deref(),
                        )
                        .map_err(|e| e.to_string());
                        let _ = tx.send(res);
                    });
                    ctx.request_repaint();
                }
            }
        }
        if close_modal {
            self.dial_prompt = None;
        }
    }

    /// Controller sessions stay allowed in outgoing-only mode (outgoing-only
    /// blocks hosting/incoming, not the local person reaching out).
    #[cfg(test)]
    pub fn start_session(&mut self) {
        if self.selected.is_none() || self.onboarding_open {
            return;
        }
        self.session_active = true;
        self.recording = true;
        self.section = AppSection::Session;
        self.push_audit("session started (local recording on)");
    }

    pub fn end_session(&mut self) {
        self.session_active = false;
        self.recording = false;
        self.frame_no = 0;
        self.texture = None;
        self.last_rect = None;
        self.last_remote = None;
        self.last_sent_pos = None;
        self.live_dead = false;
        self.source = FrameSource::Synthetic;
        self.tofu_notice = None;
        self.section = AppSection::Board;
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

    pub fn set_live(&mut self, live: LiveSource) {
        self.push_audit(&live.greeting.clone());
        self.source = FrameSource::Live(live);
        self.texture = None;
        self.live_dead = false;
        self.session_active = true;
        self.recording = true;
        self.section = AppSection::Session;
    }

    /// Switch window sections. Session needs a live session; Help and Board
    /// are always available.
    pub fn show_section(&mut self, section: AppSection) {
        if section == AppSection::Session && !self.session_active {
            return;
        }
        self.section = section;
    }

    /// Drop queued frames while the session runs in the background (the user
    /// is on the Board or Help tab). The channel is unbounded, so never
    /// letting it fill is what keeps background sessions cheap.
    fn drain_background_frames(&mut self) {
        if self.session_active && self.section != AppSection::Session {
            if let FrameSource::Live(live) = &mut self.source {
                while live.frames.try_recv().is_ok() {}
            }
        }
    }

    fn paint_frame(&mut self, ctx: &egui::Context) {
        match &mut self.source {
            FrameSource::Synthetic => {
                let (meta, px) = tethercraft_capture::fake_frame(320, 200, self.frame_no);
                self.frame_no = self.frame_no.wrapping_add(1);
                self.show_rgba(ctx, meta.width as usize, meta.height as usize, &px);
            }
            FrameSource::Live(live) => {
                let mut latest: Option<(u32, u32, Vec<u8>)> = None;
                let mut dead = false;
                loop {
                    match live.frames.try_recv() {
                        Ok(f) => latest = Some(f),
                        Err(mpsc::TryRecvError::Empty) => break,
                        Err(mpsc::TryRecvError::Disconnected) => {
                            dead = true;
                            break;
                        }
                    }
                }
                if dead {
                    self.live_dead = true;
                }
                if let Some((w, h, px)) = latest {
                    self.last_remote = Some((w, h));
                    self.show_rgba(ctx, w as usize, h as usize, &px);
                }
            }
        }
    }

    fn show_rgba(&mut self, ctx: &egui::Context, width: usize, height: usize, px: &[u8]) {
        match tethercraft_codec::to_egui_image(width, height, px) {
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

    /// Map an egui key to a Windows virtual-key code for the host.
    pub fn vk_for_key(key: egui::Key) -> Option<u16> {
        use egui::Key as K;
        let vk: u32 = match key {
            K::ArrowDown => 0x28,
            K::ArrowLeft => 0x25,
            K::ArrowRight => 0x27,
            K::ArrowUp => 0x26,
            K::Enter => 0x0D,
            K::Escape => 0x1B,
            K::Tab => 0x09,
            K::Backspace => 0x08,
            K::Space => 0x20,
            K::Delete => 0x2E,
            K::Insert => 0x2D,
            K::Home => 0x24,
            K::End => 0x23,
            K::PageUp => 0x21,
            K::PageDown => 0x22,
            K::F1 => 0x70,
            K::F2 => 0x71,
            K::F3 => 0x72,
            K::F4 => 0x73,
            K::F5 => 0x74,
            K::F6 => 0x75,
            K::F7 => 0x76,
            K::F8 => 0x77,
            K::F9 => 0x78,
            K::F10 => 0x79,
            K::F11 => 0x7A,
            K::F12 => 0x7B,
            K::A => 0x41,
            K::B => 0x42,
            K::C => 0x43,
            K::D => 0x44,
            K::E => 0x45,
            K::F => 0x46,
            K::G => 0x47,
            K::H => 0x48,
            K::I => 0x49,
            K::J => 0x4A,
            K::K => 0x4B,
            K::L => 0x4C,
            K::M => 0x4D,
            K::N => 0x4E,
            K::O => 0x4F,
            K::P => 0x50,
            K::Q => 0x51,
            K::R => 0x52,
            K::S => 0x53,
            K::T => 0x54,
            K::U => 0x55,
            K::V => 0x56,
            K::W => 0x57,
            K::X => 0x58,
            K::Y => 0x59,
            K::Z => 0x5A,
            K::Num0 => 0x30,
            K::Num1 => 0x31,
            K::Num2 => 0x32,
            K::Num3 => 0x33,
            K::Num4 => 0x34,
            K::Num5 => 0x35,
            K::Num6 => 0x36,
            K::Num7 => 0x37,
            K::Num8 => 0x38,
            K::Num9 => 0x39,
            _ => return None,
        };
        Some(vk as u16)
    }

    /// Forward local pointer and keyboard state to the host. Only while a
    /// live, non-view-only session owns the frame.
    fn forward_input(&mut self, ctx: &egui::Context) {
        let tx = match &mut self.source {
            FrameSource::Live(live) if !live.view_only => &live.input,
            _ => return,
        };
        let (rect, remote) = match (self.last_rect, self.last_remote) {
            (Some(r), Some(d)) => (r, d),
            _ => return,
        };
        if let Some(pos) = ctx.pointer_hover_pos() {
            if rect.contains(pos) && rect.size().x > 0.0 && rect.size().y > 0.0 {
                let fx = (pos.x - rect.min.x) / rect.size().x;
                let fy = (pos.y - rect.min.y) / rect.size().y;
                let rx = (fx * remote.0 as f32).round() as i32;
                let ry = (fy * remote.1 as f32).round() as i32;
                if self.last_sent_pos != Some((rx, ry)) {
                    self.last_sent_pos = Some((rx, ry));
                    let _ = tx.send(InputEvent::MouseMove { x: rx, y: ry });
                }
                if ctx.input(|i| i.pointer.primary_clicked()) {
                    let _ = tx.send(InputEvent::MouseClick { button: 0 });
                }
                if ctx.input(|i| i.pointer.secondary_clicked()) {
                    let _ = tx.send(InputEvent::MouseClick { button: 1 });
                }
                let scroll: egui::Vec2 = ctx.input(|i| i.raw_scroll_delta);
                if scroll.length() > 0.5 {
                    let _ = tx.send(InputEvent::MouseScroll {
                        dx: scroll.x.round() as i32,
                        dy: scroll.y.round() as i32,
                    });
                }
            }
        }
        let events = ctx.input(|i| i.events.clone());
        for ev in events {
            if let egui::Event::Key { key, pressed, .. } = ev {
                if let Some(vk) = Self::vk_for_key(key) {
                    let _ = tx.send(InputEvent::Key {
                        code: u32::from(vk),
                        down: pressed,
                    });
                }
            }
        }
    }
}

impl eframe::App for ControllerUi {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let (dz, reset_zoom) = ctx.input(|i| {
            let ctrl = i.modifiers.ctrl || i.modifiers.command;
            (i.zoom_delta(), ctrl && i.key_pressed(egui::Key::Num0))
        });
        if dz != 1.0 {
            self.zoom = clamp_zoom(self.zoom * dz);
        }
        if reset_zoom {
            self.zoom = 1.0;
        }
        ctx.set_zoom_factor(self.zoom.clamp(0.75, 2.0));

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
                // Window sections, fleet-style horizontal tabs.
                for tab in AppSection::ALL {
                    let enabled = tab != AppSection::Session || self.session_active;
                    if ui
                        .add_enabled(
                            enabled,
                            egui::Button::selectable(self.section == tab, tab.title()),
                        )
                        .clicked()
                    {
                        self.show_section(tab);
                    }
                }
                ui.separator();
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

        self.poll_dial_result(ctx);

        if self.outgoing_only {
            egui::TopBottomPanel::top("outgoing").show(ctx, |ui| {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    "Outgoing-only mode: incoming connections are disabled for 24 h. \
                     If someone is on the phone telling you what to do, hang up now.",
                );
            });
        }

        let mut dismiss_tofu = false;
        if let Some(notice) = &self.tofu_notice {
            egui::TopBottomPanel::top("tofu_notice").show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let key_short = &notice.key_hex[..notice.key_hex.len().min(12)];
                    ui.colored_label(
                        egui::Color32::from_rgb(180, 110, 0),
                        format!(
                            "FIRST CONTACT (TOFU): new host key ({key_short}...) pinned for {}. Verify these words out of band: {}",
                            notice.peer_id, notice.words
                        ),
                    );
                    if ui.small_button("I verified the words (dismiss)").clicked() {
                        dismiss_tofu = true;
                    }
                });
            });
        }
        if dismiss_tofu {
            self.tofu_notice = None;
        }

        egui::SidePanel::left("peers")
            .resizable(true)
            .default_width(210.0)
            .width_range(150.0..=360.0)
            .show(ctx, |ui| {
                ui.heading("Peers");
                ui.text_edit_singleline(&mut self.board.filter);
                ui.label(format!(
                    "{} of {} shown",
                    self.board.total_shown(),
                    self.board.cards.len()
                ));
                if ui.button("Connect").clicked() {
                    if let Some(id) = self.selected.clone() {
                        self.request_connect(&id);
                    }
                }
                ui.separator();
                ui.small(
                    "Click a card to select. Double-click a card (or press Connect) to start.",
                );
                if self.board.filter.is_empty() {
                    ui.small("Type above to filter by name, id, tier, or address.");
                }
            });

        // Background drain: the user can browse the Board or Help while a
        // session runs; queued frames are dropped so the channel stays empty.
        self.drain_background_frames();

        let mut pick: Option<String> = None;
        let mut go = false;
        egui::CentralPanel::default().show(ctx, |ui| {
            match self.section {
                AppSection::Help => {
                    render_help(ui, &mut self.help_topic);
                }
                AppSection::Session if self.session_active => {
                    self.paint_frame(ctx);
                    if let Some(tex) = &self.texture {
                        let size = tex.size_vec2() * self.zoom;
                        let resp = ui.image((tex.id(), size));
                        self.last_rect = Some(resp.rect);
                    }
                    if self.decode_errors > 0 {
                        ui.label(format!("decode errors: {}", self.decode_errors));
                    }
                    if self.live_dead {
                        ui.colored_label(
                            egui::Color32::YELLOW,
                            "Remote stream ended. Press End session, then Connect again.",
                        );
                    }
                    self.forward_input(ctx);
                }
                AppSection::Session => {
                    ui.label("No active session yet.");
                    ui.label("Pick a peer card and press Connect, or run:");
                    ui.monospace("tethercraft connect <peer> [--headless out.ppm]");
                }
                AppSection::Board => {
                    // Peer kanban: Ready / First contact / Needs address
                    // columns, cards sorted by name, filter from the sidebar.
                    let mut connect_id: Option<String> = None;
                    let selected = self.selected.clone();
                    let empty_book = self.board.cards.is_empty();
                    ui.columns(BoardCol::ALL.len(), |cols| {
                        for (i, col) in BoardCol::ALL.iter().enumerate() {
                            let cards = self.board.visible_in(*col);
                            let count = self.board.count_in(*col);
                            cols[i].group(|ui| {
                                ui.horizontal(|ui| {
                                    ui.strong(col.title());
                                    ui.small(format!("({count})"));
                                });
                                ui.small(col.hint());
                                ui.separator();
                                egui::ScrollArea::vertical()
                                    .id_salt(format!("board_col_{i}"))
                                    .max_height(420.0)
                                    .show(ui, |ui| {
                                        for peer in cards {
                                            let is_sel =
                                                selected.as_deref() == Some(peer.id.as_str());
                                            let dot = if peer.has_key {
                                                egui::Color32::GREEN
                                            } else {
                                                egui::Color32::YELLOW
                                            };
                                            let frame = egui::Frame::group(ui.style())
                                                .stroke(if is_sel {
                                                    egui::Stroke::new(
                                                        2.0_f32,
                                                        egui::Color32::LIGHT_BLUE,
                                                    )
                                                } else {
                                                    ui.style()
                                                        .visuals
                                                        .widgets
                                                        .noninteractive
                                                        .bg_stroke
                                                })
                                                .show(ui, |ui| {
                                                    ui.set_min_size(egui::vec2(170.0, 132.0));
                                                    ui.horizontal(|ui| {
                                                        ui.colored_label(dot, "●");
                                                        ui.strong(peer.display_name());
                                                    });
                                                    ui.monospace(&peer.id);
                                                    ui.small(peer.addr.clone().unwrap_or_else(
                                                        || "no address yet".to_string(),
                                                    ));
                                                    ui.small(format!("tier: {}", peer.tier));
                                                    ui.small(if peer.has_key {
                                                        "key pinned"
                                                    } else {
                                                        "first contact: verify words"
                                                    });
                                                    ui.horizontal(|ui| {
                                                        if ui.small_button("Connect").clicked() {
                                                            pick = Some(peer.id.clone());
                                                            go = true;
                                                        }
                                                        if ui.small_button("Copy ID").clicked() {
                                                            connect_id = Some(peer.id.clone());
                                                        }
                                                    });
                                                });
                                            let resp =
                                                frame.response.interact(egui::Sense::click());
                                            if resp.clicked() {
                                                pick = Some(peer.id.clone());
                                            }
                                            if resp.double_clicked() {
                                                pick = Some(peer.id.clone());
                                                go = true;
                                            }
                                        }
                                    });
                            });
                        }
                    });
                    if let Some(id) = connect_id {
                        ui.ctx().copy_text(id);
                    }
                    if empty_book {
                        ui.label("Address book is empty. Add a peer first:");
                        ui.monospace("tethercraft book add <peer> <host-key-hex> --addr host:port");
                    }
                }
            }
        });
        if let Some(id) = pick {
            self.select(&id);
        }
        if go {
            if let Some(id) = self.selected.clone() {
                self.request_connect(&id);
            }
        }
        self.render_dial_modal(ctx);

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

    fn card(id: &str) -> PeerCard {
        PeerCard {
            id: id.to_string(),
            alias: String::new(),
            addr: None,
            has_key: false,
            tier: "stranger".to_string(),
        }
    }

    #[test]
    fn select_start_end() {
        let mut ui = ControllerUi::new(vec![card("p1")], false, false, None);
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
        let mut ui = ControllerUi::new(vec![card("p1")], false, true, None);
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
    fn vk_mapping_is_stable() {
        assert!(ControllerUi::vk_for_key(egui::Key::A) == Some(0x41));
        assert!(ControllerUi::vk_for_key(egui::Key::Enter) == Some(0x0D));
        assert!(ControllerUi::vk_for_key(egui::Key::F5) == Some(0x74));
        assert!(ControllerUi::vk_for_key(egui::Key::Num9) == Some(0x39));
    }

    #[test]
    fn board_search_filters() {
        let mut wired = PeerCard {
            id: "goliath".to_string(),
            alias: "Goliath".to_string(),
            addr: Some("127.0.0.1:21120".to_string()),
            has_key: true,
            tier: "trusted_helper".to_string(),
        };
        assert!(wired.matches(""));
        assert!(wired.matches("gol"));
        assert!(wired.matches("127.0.0"));
        assert!(wired.matches("trusted"));
        assert!(!wired.matches("zebra"));
        wired.alias = String::new();
        assert!(wired.display_name() == "goliath");
    }

    #[test]
    fn sections_gate_session_tab() {
        let mut ui = ControllerUi::new(vec![card("p1")], false, false, None);
        assert!(ui.section == AppSection::Board);
        ui.show_section(AppSection::Session);
        assert!(ui.section == AppSection::Board);
        ui.show_section(AppSection::Help);
        assert!(ui.section == AppSection::Help);
        ui.select("p1");
        ui.start_session();
        assert!(ui.section == AppSection::Session);
        ui.end_session();
        assert!(ui.section == AppSection::Board);
    }

    #[test]
    fn audit_bounded() {
        let mut ui = ControllerUi::new(Vec::new(), false, false, None);
        for _ in 0..250 {
            ui.push_audit("x");
        }
        assert!(ui.audit.len() == 200);
    }

    #[test]
    fn request_connect_opens_dial_prompt_and_guards_unaddressed() {
        let mut ui = ControllerUi::new(
            vec![
                PeerCard {
                    id: "desk".to_string(),
                    alias: "Desk".to_string(),
                    addr: Some("127.0.0.1:21120".to_string()),
                    has_key: true,
                    tier: "managed".to_string(),
                },
                card("no-addr"),
            ],
            false,
            false,
            None,
        );
        ui.pinned_keys.insert("desk".to_string(), [5u8; 32]);
        ui.request_connect("no-addr");
        assert!(ui.dial_prompt.is_none());
        ui.request_connect("desk");
        assert!(ui.dial_prompt.is_some());
        if let Some(p) = &ui.dial_prompt {
            assert!(p.addr == "127.0.0.1:21120");
            assert!(p.expected_key == Some([5u8; 32]));
        }
        // Ending a session clears any lingering TOFU notice.
        ui.tofu_notice = Some(TofuNotice {
            peer_id: "desk".to_string(),
            words: "alpha bravo".to_string(),
            key_hex: "00".to_string(),
        });
        ui.end_session();
        assert!(ui.tofu_notice.is_none());
    }
}
