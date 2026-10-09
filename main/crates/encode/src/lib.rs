//! GIF encoding pipeline.
//!
//! Data flow: [`VideoFrame`] in, `.gif` file out. Heavy lifting is done
//! by the `gifski` crate.
//!
//! Threading model: gifski's `Writer::write` blocks until the `Collector`
//! is dropped, so [`Pipeline`] runs it on a dedicated blocking thread.
//! The async `app` runtime stays free for capture; progress arrives over
//! a plain `std::sync::mpsc` channel. No tokio needed in this crate.

pub mod estimator;
pub mod pipeline;
pub mod settings;

pub use estimator::{estimate, warn, Estimate, SizeLevel};
pub use pipeline::{encode_all, EncodeProgress, EncodeResult, Pipeline};
pub use settings::EncodeSettings;

// `VideoFrame` lives in core (shared handoff type); re-exported here
// so existing `peek_encode::VideoFrame` paths keep working.
pub use peek_core::VideoFrame;
