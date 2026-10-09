//! Raw captured frame. The handoff type between `capture` (producer)
//! and `encode` (consumer); owned by `core` so neither direction gains
//! an awkward dependency on the other.

use std::time::Duration;

use crate::peek_error::PeekError;

/// One RGBA frame. `pts` is stream time; consumers only rely on
/// relative spacing (the encode pipeline normalizes the first frame
/// to t=0).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub pts: Duration,
    /// Row-major RGBA bytes, exactly `width * height * 4` long.
    pub rgba: Vec<u8>,
}

impl VideoFrame {
    pub fn new(width: u32, height: u32, pts: Duration, rgba: Vec<u8>) -> Result<Self, PeekError> {
        if width == 0 || height == 0 {
            return Err(PeekError::InvalidFrame(format!(
                "zero dimension {width}x{height}"
            )));
        }
        let expected = width as usize * height as usize * 4;
        if rgba.len() != expected {
            return Err(PeekError::InvalidFrame(format!(
                "buffer is {} bytes, expected {expected} for {width}x{height} RGBA",
                rgba.len()
            )));
        }
        Ok(Self {
            width,
            height,
            pts,
            rgba,
        })
    }

    pub fn pixels(&self) -> u64 {
        self.width as u64 * self.height as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: u32, height: u32, v: u8) -> VideoFrame {
        VideoFrame::new(
            width,
            height,
            Duration::from_millis(0),
            vec![v; width as usize * height as usize * 4],
        )
        .unwrap()
    }

    #[test]
    fn accepts_matching_buffer() {
        let f = solid(64, 48, 128);
        assert_eq!(f.pixels(), 64 * 48);
    }

    #[test]
    fn rejects_short_buffer() {
        let err = VideoFrame::new(64, 48, Duration::ZERO, vec![0u8; 100]).unwrap_err();
        assert!(matches!(err, PeekError::InvalidFrame(_)));
    }

    #[test]
    fn rejects_zero_dimension() {
        assert!(VideoFrame::new(0, 48, Duration::ZERO, vec![]).is_err());
    }
}
