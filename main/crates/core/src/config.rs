//! Persistent user settings. `core` owns the shape and validation;
//! the `app` layer decides *where* the file lives (XDG config dir).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::fps::Fps;
use crate::peek_error::PeekError;
use crate::rect::LogicalRect;

/// gifski quality, 1..=100. 80 balances size and banding for UI captures.
pub const DEFAULT_QUALITY: u8 = 80;
/// Downscale factor applied before encode, 0.25..=1.0.
pub const DEFAULT_SCALE: f32 = 1.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    /// `None` means "resolve platform default" (~/Videos/Peek).
    /// Kept optional so `core` needs no directories dependency.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_dir: Option<PathBuf>,
    #[serde(default)]
    pub fps: Fps,
    #[serde(default = "default_quality")]
    pub quality: u8,
    #[serde(default = "default_scale")]
    pub scale: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_rect: Option<LogicalRect>,
    /// Portal restore token for the picked monitor. With this stored,
    /// later recordings restore the choice silently instead of popping
    /// the system picker every time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restore_token: Option<String>,
}

fn default_quality() -> u8 {
    DEFAULT_QUALITY
}

fn default_scale() -> f32 {
    DEFAULT_SCALE
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            output_dir: None,
            fps: Fps::default(),
            quality: DEFAULT_QUALITY,
            scale: DEFAULT_SCALE,
            last_rect: None,
            restore_token: None,
        }
    }
}

impl AppConfig {
    pub fn validate(&self) -> Result<(), PeekError> {
        if !(1..=100).contains(&self.quality) {
            return Err(PeekError::InvalidQuality(self.quality));
        }
        if !(0.25..=1.0).contains(&self.scale) {
            return Err(PeekError::InvalidScale(self.scale));
        }
        Ok(())
    }

    pub fn from_toml_str(s: &str) -> Result<Self, PeekError> {
        let cfg: Self = toml::from_str(s).map_err(|e| PeekError::ConfigParse(e.to_string()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn to_toml_string(&self) -> Result<String, PeekError> {
        self.validate()?;
        toml::to_string_pretty(self).map_err(|e| PeekError::ConfigParse(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        AppConfig::default().validate().unwrap();
    }

    #[test]
    fn rejects_bad_quality_and_scale() {
        let mut c = AppConfig::default();
        c.quality = 0;
        assert_eq!(c.validate(), Err(PeekError::InvalidQuality(0)));
        c.quality = 80;
        c.scale = 2.0;
        assert_eq!(c.validate(), Err(PeekError::InvalidScale(2.0)));
    }

    #[test]
    fn toml_round_trip() {
        let mut c = AppConfig::default();
        c.fps = Fps::Fps30;
        c.last_rect = Some(LogicalRect::new(10, 20, 694, 494).unwrap());
        let s = c.to_toml_string().unwrap();
        assert!(s.contains("fps = 30"));
        let back = AppConfig::from_toml_str(&s).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn bad_toml_surfaces_parse_error() {
        // serde wraps the inner `InvalidFps` inside toml's error type,
        // so it surfaces as ConfigParse with the message preserved.
        let err = AppConfig::from_toml_str("fps = 24").unwrap_err();
        match err {
            PeekError::ConfigParse(msg) => {
                assert!(msg.contains("invalid fps 24"), "unexpected msg: {msg}");
            }
            other => panic!("expected ConfigParse, got {other:?}"),
        }
    }
}
