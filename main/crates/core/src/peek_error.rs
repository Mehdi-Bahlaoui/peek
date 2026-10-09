use thiserror::Error;

/// Every failure in the workspace funnels through this type at the
/// crate boundary. UI-facing code matches on variants, never on strings.
#[derive(Debug, Error, PartialEq)]
pub enum PeekError {
    #[error("invalid rect: {0}")]
    InvalidRect(String),

    #[error("invalid fps {0}: expected one of 10, 15, 30")]
    InvalidFps(u8),

    #[error("invalid quality {0}: expected 1..=100")]
    InvalidQuality(u8),

    #[error("invalid scale {0:?}: expected 0.25..=1.0")]
    InvalidScale(f32),

    #[error("invalid monitor scale {0:?}: expected 1.0..=4.0")]
    InvalidMonitorScale(f64),

    #[error("config parse error: {0}")]
    ConfigParse(String),

    #[error("io error: {0}")]
    Io(String),

    #[error("invalid frame: {0}")]
    InvalidFrame(String),

    #[error("encode error: {0}")]
    Encode(String),

    #[error("capture error: {0}")]
    Capture(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_messages_are_stable() {
        assert_eq!(
            PeekError::InvalidFps(24).to_string(),
            "invalid fps 24: expected one of 10, 15, 30"
        );
        assert_eq!(
            PeekError::InvalidQuality(0).to_string(),
            "invalid quality 0: expected 1..=100"
        );
    }
}
