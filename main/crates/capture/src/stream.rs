//! PipeWire frames via GStreamer. `pipewiresrc` consumes the portal
//! fd + node id; `videoconvert` + `capsfilter` normalize to RGBA
//! (PipeWire otherwise negotiates BGRx, which silently swaps red and
//! blue); `appsink` hands buffers to us on demand. Pull-based (no signal
//! callbacks), so the capture loop owns the fps cadence and slow encode
//! never blocks it: `drop=true` discards stale frames instead of
//! queueing them.

use std::os::fd::{AsRawFd, OwnedFd};
use std::time::Duration;

use gstreamer::prelude::*;
use gstreamer_app::AppSink;
use peek_core::{PeekError, VideoFrame};

/// First-frame wait: 100 × 50ms before giving up on a dead stream.
const FIRST_FRAME_TRIES: usize = 100;

pub struct PipeWireSource {
    pipeline: gstreamer::Pipeline,
    sink: AppSink,
    pub width: u32,
    pub height: u32,
    /// The frame that arrived while starting: real content, guaranteed.
    /// Kept for the first detection attempt because the screencast
    /// stream is damage-driven — a still screen may never deliver
    /// another frame.
    first_frame: Option<VideoFrame>,
}

fn cap<T, E: std::fmt::Debug>(r: Result<T, E>, what: &str) -> Result<T, PeekError> {
    r.map_err(|e| PeekError::Capture(format!("{what}: {e:?}")))
}

impl PipeWireSource {
    /// Build `pipewiresrc(fd, node) ! videoconvert ! appsink`, start it,
    /// and block (with timeout) for the first frame to learn the real
    /// stream dimensions.
    pub async fn start(fd: &OwnedFd, node_id: u32) -> Result<Self, PeekError> {
        cap(gstreamer::init(), "init gstreamer")?;

        let src = cap(
            gstreamer::ElementFactory::make("pipewiresrc")
                .name("src")
                .build(),
            "make pipewiresrc (is the pipewire gstreamer plugin installed?)",
        )?;
        src.set_property("fd", fd.as_raw_fd());
        // `path` is a gchararray on GstPipeWireSrc: the node id as text.
        src.set_property("path", node_id.to_string());

        let convert = cap(
            gstreamer::ElementFactory::make("videoconvert")
                .name("convert")
                .build(),
            "make videoconvert",
        )?;
        // Pin the pixel format: without this, negotiation can settle on
        // BGRx and every consumer assumption about RGBA is silently
        // wrong (marker detection, crop, GIF encode all swap red/blue).
        let rgba_caps = gstreamer::Caps::builder("video/x-raw")
            .field("format", "RGBA")
            .build();
        let filter = cap(
            gstreamer::ElementFactory::make("capsfilter")
                .name("rgba")
                .property("caps", &rgba_caps)
                .build(),
            "make RGBA capsfilter",
        )?;
        let sink_elem = cap(
            gstreamer::ElementFactory::make("appsink")
                .name("sink")
                .build(),
            "make appsink",
        )?;
        sink_elem.set_property("max-buffers", 3u32);
        sink_elem.set_property("drop", true);
        sink_elem.set_property("sync", false);

        let pipeline = gstreamer::Pipeline::new();
        cap(
            pipeline.add_many([&src, &convert, &filter, &sink_elem]),
            "assemble pipeline",
        )?;
        cap(
            gstreamer::Element::link_many([&src, &convert, &filter, &sink_elem]),
            "link pipeline",
        )?;
        let sink = sink_elem
            .downcast::<AppSink>()
            .map_err(|_| PeekError::Capture("appsink downcast failed".into()))?;

        cap(
            pipeline.set_state(gstreamer::State::Playing),
            "start pipeline",
        )?;

        let mut first: Option<VideoFrame> = None;
        for _ in 0..FIRST_FRAME_TRIES {
            if let Some(frame) = pull_one(&sink)? {
                first = Some(frame);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let first = first
            .ok_or_else(|| PeekError::Capture("no data from pipewire stream after 5s".into()))?;
        let (width, height) = (first.width, first.height);

        Ok(Self {
            pipeline,
            sink,
            width,
            height,
            first_frame: Some(first),
        })
    }

    /// Non-blocking: newest buffered frame, or `None` if the stream has
    /// nothing fresh since the last call.
    pub fn pull_frame(&self) -> Result<Option<VideoFrame>, PeekError> {
        pull_one(&self.sink)
    }

    /// The startup frame consumed while waiting for the stream. Always
    /// real content; callers use it before waiting on further frames.
    pub fn take_first_frame(&mut self) -> Option<VideoFrame> {
        self.first_frame.take()
    }

    pub fn stop(&self) {
        let _ = self.pipeline.set_state(gstreamer::State::Null);
    }
}

impl Drop for PipeWireSource {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Single non-blocking appsink pull, converted to a validated frame.
fn pull_one(sink: &AppSink) -> Result<Option<VideoFrame>, PeekError> {
    let sample = match sink.try_pull_sample(gstreamer::ClockTime::ZERO) {
        Some(s) => s,
        None => return Ok(None),
    };
    let caps = sample
        .caps()
        .ok_or_else(|| PeekError::Capture("sample has no caps".into()))?;
    let structure = caps
        .structure(0)
        .ok_or_else(|| PeekError::Capture("sample caps have no structure".into()))?;
    let format: String = cap(structure.get("format"), "read frame format from caps")?;
    if format != "RGBA" {
        return Err(PeekError::Capture(format!(
            "stream delivered {format} frames, expected RGBA (pipeline negotiation bug)"
        )));
    }
    let width: i32 = cap(structure.get("width"), "read frame width from caps")?;
    let height: i32 = cap(structure.get("height"), "read frame height from caps")?;
    if width <= 0 || height <= 0 {
        return Err(PeekError::Capture(format!(
            "stream reports degenerate size {width}x{height}"
        )));
    }
    let buffer = sample
        .buffer()
        .ok_or_else(|| PeekError::Capture("sample has no buffer".into()))?;
    let map = cap(buffer.map_readable(), "map stream buffer")?;
    let bytes = map.as_slice();
    let expected = width as usize * height as usize * 4;
    if bytes.len() != expected {
        return Err(PeekError::Capture(format!(
            "buffer is {} bytes, expected {expected} for {width}x{height} RGBA (stride mismatch?)",
            bytes.len()
        )));
    }
    let pts = buffer
        .pts()
        .map(|t| Duration::from_nanos(t.nseconds()))
        .unwrap_or(Duration::ZERO);
    Ok(Some(VideoFrame::new(
        width as u32,
        height as u32,
        pts,
        bytes.to_vec(),
    )?))
}
