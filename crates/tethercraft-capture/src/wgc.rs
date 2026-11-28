//! Windows Graphics Capture (WGC, primary monitor): both persistent streaming
//! capture (`WgcStream`) and one-frame grabs (`grab_primary`).
//!
//! `WgcStream` keeps a single WGC session alive and overwrites a single
//! latest-frame BGRA slot so intermediate raw frames are dropped pre-encode
//! without paying the ~400 ms per-frame session setup/teardown cost.

use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use tethercraft_core::TetherError;
use windows_capture::capture::{CaptureControl, Context, GraphicsCaptureApiHandler};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};

/// Convert BGRA8 pixels to RGBA8 in place.
pub fn bgra_to_rgba_in_place(buf: &mut [u8]) {
    let mut i = 0;
    while i + 4 <= buf.len() {
        buf.swap(i, i + 2);
        i += 4;
    }
}

struct LatestSlot {
    seq: u64,
    width: u32,
    height: u32,
    bgra: Vec<u8>,
    closed: bool,
}

struct StreamHandler {
    shared: Arc<(Mutex<LatestSlot>, Condvar)>,
    width: u32,
    height: u32,
    expected: usize,
}

type CaptureErr = Box<dyn std::error::Error + Send + Sync>;

impl GraphicsCaptureApiHandler for StreamHandler {
    type Flags = (Arc<(Mutex<LatestSlot>, Condvar)>, u32, u32, usize);
    type Error = CaptureErr;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        let (shared, width, height, expected) = ctx.flags;
        Ok(Self {
            shared,
            width,
            height,
            expected,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        _control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let mut fb = frame.buffer()?;
        let buf = fb.as_nopadding_buffer()?;
        if buf.len() == self.expected {
            let (lock, cvar) = &*self.shared;
            if let Ok(mut slot) = lock.lock() {
                slot.bgra.clear();
                slot.bgra.extend_from_slice(buf);
                slot.width = self.width;
                slot.height = self.height;
                slot.seq = slot.seq.wrapping_add(1);
                cvar.notify_one();
            }
        }
        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        let (lock, cvar) = &*self.shared;
        if let Ok(mut slot) = lock.lock() {
            slot.closed = true;
            cvar.notify_all();
        }
        Ok(())
    }
}

/// Persistent WGC capture session on the primary monitor.
/// Delivers BGRA8 frames with latest-frame-wins pre-encode coalescing.
pub struct WgcStream {
    control: Option<CaptureControl<StreamHandler, CaptureErr>>,
    shared: Arc<(Mutex<LatestSlot>, Condvar)>,
    last_seq: u64,
}

impl WgcStream {
    pub fn start() -> Result<Self, TetherError> {
        let monitor =
            Monitor::primary().map_err(|e| TetherError::Invalid(format!("wgc: {e:?}")))?;
        let width = monitor
            .width()
            .map_err(|e| TetherError::Invalid(format!("wgc: {e:?}")))?;
        let height = monitor
            .height()
            .map_err(|e| TetherError::Invalid(format!("wgc: {e:?}")))?;
        if width == 0 || height == 0 || width > 7680 || height > 4320 {
            return Err(TetherError::Invalid("wgc: bad monitor size".to_string()));
        }
        let expected = width as usize * height as usize * 4;
        let shared = Arc::new((
            Mutex::new(LatestSlot {
                seq: 0,
                width,
                height,
                bgra: Vec::new(),
                closed: false,
            }),
            Condvar::new(),
        ));
        let settings = Settings::new(
            monitor,
            CursorCaptureSettings::Default,
            DrawBorderSettings::Default,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Default,
            DirtyRegionSettings::Default,
            ColorFormat::Bgra8,
            (shared.clone(), width, height, expected),
        );
        let control = StreamHandler::start_free_threaded(settings)
            .map_err(|e| TetherError::Invalid(format!("wgc start: {e:?}")))?;
        Ok(Self {
            control: Some(control),
            shared,
            last_seq: 0,
        })
    }

    /// Wait up to `timeout` for the next unseen BGRA8 frame.
    /// Returns `Ok(None)` if the screen did not repaint within `timeout`.
    pub fn next_frame(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<(u32, u32, Vec<u8>)>, TetherError> {
        let (lock, cvar) = &*self.shared;
        let mut guard = lock
            .lock()
            .map_err(|_| TetherError::Invalid("wgc: lock poisoned".to_string()))?;
        let deadline = Instant::now() + timeout;
        while guard.seq == self.last_seq && !guard.closed {
            let now = Instant::now();
            if now >= deadline {
                return Ok(None);
            }
            let (g, wait_res) = cvar
                .wait_timeout(guard, deadline - now)
                .map_err(|_| TetherError::Invalid("wgc: lock poisoned".to_string()))?;
            guard = g;
            if wait_res.timed_out() && guard.seq == self.last_seq {
                return Ok(None);
            }
        }
        if guard.seq != self.last_seq {
            self.last_seq = guard.seq;
            let w = guard.width;
            let h = guard.height;
            let bgra = std::mem::take(&mut guard.bgra);
            return Ok(Some((w, h, bgra)));
        }
        Err(TetherError::Invalid(
            "wgc: capture stream closed".to_string(),
        ))
    }
}

impl Drop for WgcStream {
    fn drop(&mut self) {
        if let Some(ctrl) = self.control.take() {
            let _ = ctrl.stop();
        }
    }
}

struct OneShot {
    tx: mpsc::Sender<Vec<u8>>,
    expected: usize,
}

impl GraphicsCaptureApiHandler for OneShot {
    type Flags = (mpsc::Sender<Vec<u8>>, usize);
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        let (tx, expected) = ctx.flags;
        Ok(Self { tx, expected })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let mut fb = frame.buffer()?;
        let buf = fb.as_nopadding_buffer()?;
        if buf.len() == self.expected {
            // Bgra8 -> Rgba8: swap R and B in place on a copy.
            let mut rgba = buf.to_vec();
            bgra_to_rgba_in_place(&mut rgba);
            let _ = self.tx.send(rgba);
        }
        control.stop();
        Ok(())
    }
}

/// Grab one primary-monitor frame as RGBA. Real WGC; fails honestly on
/// headless machines, in VMs without a virtual display, or on timeout.
pub fn grab_primary(timeout: Duration) -> Result<(u32, u32, Vec<u8>), TetherError> {
    let monitor = Monitor::primary().map_err(|e| TetherError::Invalid(format!("wgc: {e:?}")))?;
    let width = monitor
        .width()
        .map_err(|e| TetherError::Invalid(format!("wgc: {e:?}")))?;
    let height = monitor
        .height()
        .map_err(|e| TetherError::Invalid(format!("wgc: {e:?}")))?;
    if width == 0 || height == 0 || width > 7680 || height > 4320 {
        return Err(TetherError::Invalid("wgc: bad monitor size".to_string()));
    }
    let expected = width as usize * height as usize * 4;
    let (tx, rx) = mpsc::channel();
    let settings = Settings::new(
        monitor,
        CursorCaptureSettings::Default,
        DrawBorderSettings::Default,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Bgra8,
        (tx, expected),
    );
    let control = OneShot::start_free_threaded(settings)
        .map_err(|e| TetherError::Invalid(format!("wgc start: {e:?}")))?;
    let deadline = Instant::now() + timeout;
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(rgba) => {
                let _ = control.stop();
                if rgba.len() != expected {
                    return Err(TetherError::Invalid("wgc: short frame".to_string()));
                }
                return Ok((width, height, rgba));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if Instant::now() >= deadline {
                    let _ = control.stop();
                    return Err(TetherError::Invalid("wgc: no frame in time".to_string()));
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = control.stop();
                return Err(TetherError::Invalid("wgc: capture ended".to_string()));
            }
        }
    }
}
