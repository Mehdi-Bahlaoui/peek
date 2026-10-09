//! Frame rate. GIF has no motion compensation, so each extra fps
//! linearly grows file size — hence exactly three steps.

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::peek_error::PeekError;

/// Allowed capture rates. 10fps is the default (Peek parity);
/// 30fps is supported with a size warning from `encode::estimator`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Fps {
    #[default]
    Fps10,
    Fps15,
    Fps30,
}

impl Fps {
    pub const fn value(self) -> u8 {
        match self {
            Self::Fps10 => 10,
            Self::Fps15 => 15,
            Self::Fps30 => 30,
        }
    }

    pub fn from_u8(v: u8) -> Result<Self, PeekError> {
        match v {
            10 => Ok(Self::Fps10),
            15 => Ok(Self::Fps15),
            30 => Ok(Self::Fps30),
            other => Err(PeekError::InvalidFps(other)),
        }
    }

    /// Nominal delay between frames.
    pub fn frame_interval(self) -> Duration {
        Duration::from_secs_f64(1.0 / f64::from(self.value()))
    }
}

impl fmt::Display for Fps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}fps", self.value())
    }
}

// Stored as a plain integer in TOML (`fps = 15`), validated on load.
impl Serialize for Fps {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u8(self.value())
    }
}

impl<'de> Deserialize<'de> for Fps {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = u8::deserialize(d)?;
        Self::from_u8(v).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_all_rates() {
        for v in [10u8, 15, 30] {
            assert_eq!(Fps::from_u8(v).unwrap().value(), v);
        }
        assert_eq!(Fps::from_u8(24), Err(PeekError::InvalidFps(24)));
    }

    #[test]
    fn intervals_shrink_with_rate() {
        assert!(Fps::Fps30.frame_interval() < Fps::Fps10.frame_interval());
        assert_eq!(Fps::Fps15.frame_interval().as_millis(), 66);
    }
}
