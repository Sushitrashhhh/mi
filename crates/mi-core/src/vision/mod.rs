pub mod capture;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum VisionError {
    #[error("no monitor found to capture")]
    NoMonitor,
    #[error("no focused window to capture")]
    NoWindow,
    #[error("screen capture failed: {0}")]
    Capture(String),
    #[error("image encode failed: {0}")]
    Encode(String),
}

/// What a screen capture should grab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureTarget {
    /// The whole primary display.
    PrimaryMonitor,
    /// The currently focused window.
    ActiveWindow,
}
