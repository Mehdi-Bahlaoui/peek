//! Orchestration: owns the state machine, the capture loop, and the
//! encode worker. GTK runs the main thread; this crate's async tasks
//! ride a Tokio runtime and talk to the UI over channels.

pub mod orchestrator;
pub mod state;

pub use orchestrator::{load_config, run, save_config};
pub use state::{dest_path, encode_settings, AppState};
