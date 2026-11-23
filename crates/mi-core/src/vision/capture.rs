use std::io::Cursor;

use base64::Engine;
use image::RgbaImage;
use xcap::{Monitor, Window};

use super::{CaptureTarget, VisionError};

/// A captured screenshot held in memory as PNG bytes. Never written to disk
/// unless the caller explicitly chooses to (privacy default: keep_screenshots = false).
pub struct Screenshot {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl Screenshot {
    /// base64-encoded PNG, ready for a Gemini `inline_data` part.
    pub fn to_base64(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(&self.png)
    }

    pub fn mime_type(&self) -> &'static str {
        "image/png"
    }
}

/// Capture a screenshot of the given target, returning PNG bytes in memory.
pub fn capture(target: CaptureTarget) -> Result<Screenshot, VisionError> {
    let image = match target {
        CaptureTarget::PrimaryMonitor => capture_primary_monitor()?,
        CaptureTarget::ActiveWindow => capture_active_window()?,
    };
    encode_png(image)
}

fn capture_primary_monitor() -> Result<RgbaImage, VisionError> {
    let monitors = Monitor::all().map_err(|e| VisionError::Capture(e.to_string()))?;
    if monitors.is_empty() {
        return Err(VisionError::NoMonitor);
    }

    // Prefer the primary monitor; fall back to the first one.
    let monitor = monitors
        .iter()
        .find(|m| m.is_primary().unwrap_or(false))
        .unwrap_or(&monitors[0]);

    monitor
        .capture_image()
        .map_err(|e| VisionError::Capture(e.to_string()))
}

fn capture_active_window() -> Result<RgbaImage, VisionError> {
    let windows = Window::all().map_err(|e| VisionError::Capture(e.to_string()))?;

    let window = windows
        .iter()
        .find(|w| w.is_focused().unwrap_or(false))
        .ok_or(VisionError::NoWindow)?;

    window
        .capture_image()
        .map_err(|e| VisionError::Capture(e.to_string()))
}

fn encode_png(image: RgbaImage) -> Result<Screenshot, VisionError> {
    let width = image.width();
    let height = image.height();

    let mut buf = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut buf, image::ImageFormat::Png)
        .map_err(|e| VisionError::Encode(e.to_string()))?;

    Ok(Screenshot {
        png: buf.into_inner(),
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Requires a real display, so it's ignored by default (headless CI has none).
    // Run with: cargo test -p mi-core vision -- --ignored --nocapture
    #[test]
    #[ignore]
    fn test_capture_primary_monitor_produces_png() {
        let shot = capture(CaptureTarget::PrimaryMonitor).expect("capture failed");
        assert!(shot.width > 0 && shot.height > 0);
        // PNG magic number.
        assert_eq!(&shot.png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        // base64 of a real screenshot is substantial.
        assert!(shot.to_base64().len() > 1000);
    }
}
