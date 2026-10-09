//! Recorder state plus the pure helpers around it: output paths,
//! settings translation, snapshot rendering. All unit-tested; the
//! async side effects live in `orchestrator`.

use std::path::PathBuf;

use chrono::Local;
use peek_core::AppConfig;
use peek_encode::EncodeSettings;
use peek_ui::{UiSnapshot, UiState};

#[derive(Debug, Clone)]
pub enum AppState {
    Selecting,
    Recording,
    Encoding,
    Done,
    Failed,
}

impl AppState {
    pub fn ui_state(&self) -> UiState {
        match self {
            Self::Selecting => UiState::Selecting,
            Self::Recording => UiState::Recording,
            Self::Encoding => UiState::Encoding,
            Self::Done => UiState::Done,
            Self::Failed => UiState::Failed,
        }
    }

    pub fn snapshot(&self) -> UiSnapshot {
        UiSnapshot {
            state: self.ui_state(),
        }
    }
}

/// Translate user config into encode settings.
pub fn encode_settings(config: &AppConfig) -> EncodeSettings {
    EncodeSettings {
        fps: config.fps,
        quality: config.quality,
        fast: false,
        scale: config.scale,
    }
}

/// `~/Videos/Peek/Peek 2026-07-15 08-43.gif`, falling back to the
/// current directory when no video dir is known. Caller creates parents.
pub fn dest_path(video_dir: Option<PathBuf>) -> PathBuf {
    let dir = video_dir.unwrap_or_else(|| PathBuf::from("."));
    let name = Local::now().format("Peek %Y-%m-%d %H-%M.gif").to_string();
    dir.join(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use peek_core::Fps;

    #[test]
    fn dest_name_matches_peek_convention() {
        let path = dest_path(Some(PathBuf::from("/tmp")));
        let name = path.file_name().unwrap().to_string_lossy();
        assert!(name.starts_with("Peek "));
        assert!(name.ends_with(".gif"));
        assert_eq!(path.parent().unwrap(), std::path::Path::new("/tmp"));
    }

    #[test]
    fn settings_follow_config() {
        let mut config = AppConfig::default();
        config.fps = Fps::Fps30;
        config.quality = 60;
        let settings = encode_settings(&config);
        assert_eq!(settings.fps, Fps::Fps30);
        assert_eq!(settings.quality, 60);
        assert!(!settings.fast);
    }

    #[test]
    fn states_map_to_ui_states() {
        assert_eq!(AppState::Selecting.snapshot().state, UiState::Selecting);
        assert_eq!(AppState::Recording.snapshot().state, UiState::Recording);
        assert_eq!(AppState::Encoding.snapshot().state, UiState::Encoding);
        assert_eq!(AppState::Done.snapshot().state, UiState::Done);
        assert_eq!(AppState::Failed.snapshot().state, UiState::Failed);
    }
}
