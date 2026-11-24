//! Windows Graphics Capture one-frame grabs (WGC, primary monitor).
//!
//! WGC is callback-based, so the handler forwards the first frame over a
//! channel and stops. Dimensions come from the monitor upfront because the
//! frame type does not expose them; the byte count is verified on receipt.

use std::sync::mpsc;
use std::time::{Duration, Instant};
use tethercraft_core::TetherError;
use windows_capture::capture::{Context, GraphicsCaptureApiHandler};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};

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
            let mut i = 0;
            while i + 4 <= rgba.len() {
                rgba.swap(i, i + 2);
                i += 4;
            }
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
