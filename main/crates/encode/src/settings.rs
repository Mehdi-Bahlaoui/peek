//! Encode-side settings. Mirrors the user-facing [`peek_core::AppConfig`]
//! fields relevant to GIF output, plus `fast` for previews/tests.

use peek_core::{Fps, PeekError};

/// Downscale factor range, shared with `AppConfig`.
const MIN_SCALE: f32 = 0.25;
const MAX_SCALE: f32 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EncodeSettings {
    pub fps: Fps,
    pub quality: u8,
    /// Lower quality, much faster encode. Used for tests and previews.
    pub fast: bool,
    /// Uniform downscale applied before encode, 0.25..=1.0.
    pub scale: f32,
}

impl Default for EncodeSettings {
    fn default() -> Self {
        Self {
            fps: Fps::default(),
            quality: 80,
            fast: false,
            scale: 1.0,
        }
    }
}

impl EncodeSettings {
    pub fn validate(&self) -> Result<(), PeekError> {
        if !(1..=100).contains(&self.quality) {
            return Err(PeekError::InvalidQuality(self.quality));
        }
        if !(MIN_SCALE..=MAX_SCALE).contains(&self.scale) {
            return Err(PeekError::InvalidScale(self.scale));
        }
        Ok(())
    }

    /// Output dimensions after downscale. gifski resamples internally
    /// to exactly this size, so aspect is preserved (uniform factor).
    pub fn effective_dims(&self, width: u32, height: u32) -> (u32, u32) {
        let w = ((width as f32 * self.scale).round() as u32).max(2);
        let h = ((height as f32 * self.scale).round() as u32).max(2);
        (w, h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        EncodeSettings::default().validate().unwrap();
    }

    #[test]
    fn rejects_out_of_range() {
        let mut s = EncodeSettings::default();
        s.quality = 101;
        assert_eq!(s.validate(), Err(PeekError::InvalidQuality(101)));
        s.quality = 80;
        s.scale = 0.1;
        assert_eq!(s.validate(), Err(PeekError::InvalidScale(0.1)));
    }

    #[test]
    fn downscale_halves_reference_clip() {
        let s = EncodeSettings {
            scale: 0.5,
            ..EncodeSettings::default()
        };
        assert_eq!(s.effective_dims(694, 494), (347, 247));
        assert_eq!(
            EncodeSettings::default().effective_dims(694, 494),
            (694, 494)
        );
    }
}
