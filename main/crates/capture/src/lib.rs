//! Portal + PipeWire capture.
//!
//! `portal` negotiates the GNOME screen picker and yields a PipeWire
//! node; `stream` pulls RGBA frames from it via GStreamer; `crop` cuts
//! the recording rect out of each full-monitor frame; `locate` finds
//! our own window in stream frames, which is how the single Peek window
//! defines its area on Wayland (clients never learn their position any
//! other way). No screenshots involved: the marker is detected in live
//! stream frames, so recording starts with no shutter flash.

pub mod crop;
pub mod locate;
pub mod portal;
pub mod stream;

pub use crop::crop_rgba;
pub use locate::{find_frame_rgba, Detected};
pub use portal::{start_monitor_cast, CastOutcome, CastSession, MonitorStream};
pub use stream::PipeWireSource;
