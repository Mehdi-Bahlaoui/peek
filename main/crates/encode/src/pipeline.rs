//! Streaming GIF writer.
//!
//! gifski's `Writer::write` blocks until the `Collector` is dropped, so
//! the writer runs on a dedicated blocking thread spawned on the first
//! pushed frame. The caller (capture loop) just calls [`Pipeline::push_frame`]
//! and never waits on encode. Dropping an unfinished `Pipeline` is safe:
//! the collector is dropped with it, which unblocks the writer thread.

use std::fs::{self, File};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::thread::{self, JoinHandle};

use imgref::ImgVec;
use peek_core::{PeekError, VideoFrame};
use rgb::RGBA8;

use crate::settings::EncodeSettings;

/// Progress snapshots forwarded from the writer thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncodeProgress {
    pub frames_done: usize,
    pub bytes_so_far: u64,
}

/// Final output stats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodeResult {
    pub path: PathBuf,
    pub bytes: u64,
    pub frames: usize,
}

struct ChannelReporter {
    tx: Option<Sender<EncodeProgress>>,
    frames_done: usize,
    bytes_so_far: u64,
}

impl ChannelReporter {
    fn emit(&mut self) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(EncodeProgress {
                frames_done: self.frames_done,
                bytes_so_far: self.bytes_so_far,
            });
        }
    }
}

impl gifski::progress::ProgressReporter for ChannelReporter {
    fn increase(&mut self) -> bool {
        self.frames_done += 1;
        self.emit();
        true
    }

    fn written_bytes(&mut self, bytes: u64) {
        self.bytes_so_far = bytes;
        self.emit();
    }
}

struct Session {
    collector: gifski::Collector,
    writer: JoinHandle<gifski::GifResult<()>>,
}

pub struct Pipeline {
    settings: EncodeSettings,
    dest: PathBuf,
    /// `dest` with a `.part` extension; renamed on success so a crash
    /// never leaves a half-written `.gif` behind.
    part: PathBuf,
    progress_tx: Option<Sender<EncodeProgress>>,
    session: Option<Session>,
    frames_pushed: usize,
    t0: Option<std::time::Duration>,
}

impl Pipeline {
    pub fn new(
        settings: EncodeSettings,
        dest: PathBuf,
        progress_tx: Option<Sender<EncodeProgress>>,
    ) -> Result<Self, PeekError> {
        settings.validate()?;
        let part = dest.with_extension("part");
        Ok(Self {
            settings,
            dest,
            part,
            progress_tx,
            session: None,
            frames_pushed: 0,
            t0: None,
        })
    }

    pub fn push_frame(&mut self, frame: &VideoFrame) -> Result<(), PeekError> {
        if self.session.is_none() {
            self.start_session(frame)?;
        }
        let session = self.session.as_ref().expect("session just started");
        let t0 = *self.t0.get_or_insert(frame.pts);
        let pts = frame.pts.saturating_sub(t0).as_secs_f64();
        let (chunks, _) = frame.rgba.as_chunks::<4>();
        let pixels: Vec<RGBA8> = chunks
            .iter()
            .map(|p| RGBA8::new(p[0], p[1], p[2], p[3]))
            .collect();
        let img = ImgVec::new(pixels, frame.width as usize, frame.height as usize);
        session
            .collector
            .add_frame_rgba(self.frames_pushed, img, pts)
            .map_err(|e| PeekError::Encode(e.to_string()))?;
        self.frames_pushed += 1;
        Ok(())
    }

    pub fn finish(self) -> Result<EncodeResult, PeekError> {
        let session = self
            .session
            .ok_or_else(|| PeekError::Encode("finish called with no frames pushed".into()))?;
        // Drop the collector first: this is what unblocks Writer::write.
        drop(session.collector);
        session
            .writer
            .join()
            .map_err(|_| PeekError::Encode("writer thread panicked".into()))?
            .map_err(|e| PeekError::Encode(e.to_string()))?;
        fs::rename(&self.part, &self.dest).map_err(|e| PeekError::Io(e.to_string()))?;
        let bytes = fs::metadata(&self.dest)
            .map_err(|e| PeekError::Io(e.to_string()))?
            .len();
        Ok(EncodeResult {
            path: self.dest,
            bytes,
            frames: self.frames_pushed,
        })
    }

    fn start_session(&mut self, first: &VideoFrame) -> Result<(), PeekError> {
        let (width, height) = self.settings.effective_dims(first.width, first.height);
        let (collector, writer) = gifski::new(gifski::Settings {
            width: Some(width),
            height: Some(height),
            quality: self.settings.quality,
            fast: self.settings.fast,
            repeat: gifski::Repeat::Infinite,
        })
        .map_err(|e| PeekError::Encode(e.to_string()))?;
        let file = File::create(&self.part).map_err(|e| PeekError::Io(e.to_string()))?;
        let tx = self.progress_tx.take();
        let handle = thread::spawn(move || {
            let mut reporter = ChannelReporter {
                tx,
                frames_done: 0,
                bytes_so_far: 0,
            };
            writer.write(file, &mut reporter)
        });
        self.session = Some(Session {
            collector,
            writer: handle,
        });
        Ok(())
    }
}

/// One-shot helper: encode a complete frame slice to `dest`.
pub fn encode_all(
    frames: &[VideoFrame],
    settings: EncodeSettings,
    dest: PathBuf,
    progress_tx: Option<Sender<EncodeProgress>>,
) -> Result<EncodeResult, PeekError> {
    let mut pipe = Pipeline::new(settings, dest, progress_tx)?;
    for frame in frames {
        pipe.push_frame(frame)?;
    }
    pipe.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn solid(width: u32, height: u32, shade: u8, index: usize) -> VideoFrame {
        VideoFrame::new(
            width,
            height,
            Duration::from_secs_f64(index as f64 / 15.0),
            vec![shade; width as usize * height as usize * 4],
        )
        .unwrap()
    }

    fn temp_dest(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("peek-enc-test-{}-{tag}.gif", std::process::id()))
    }

    #[test]
    fn encodes_small_clip_to_gif() {
        let dest = temp_dest("clip");
        let _ = fs::remove_file(dest.with_extension("part"));
        let _ = fs::remove_file(&dest);
        let frames: Vec<VideoFrame> = (0..6).map(|i| solid(96, 72, (i * 40) as u8, i)).collect();
        let settings = EncodeSettings {
            quality: 60,
            fast: true,
            ..EncodeSettings::default()
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let result = encode_all(&frames, settings, dest.clone(), Some(tx)).unwrap();
        assert_eq!(result.frames, 6);
        assert_eq!(result.path, dest);
        assert!(result.bytes > 0);
        assert!(rx.iter().count() > 0, "expected progress events");
        fs::remove_file(&dest).unwrap();
    }

    #[test]
    fn finish_without_frames_errors() {
        let dest = temp_dest("empty");
        let pipe = Pipeline::new(EncodeSettings::default(), dest, None).unwrap();
        assert!(matches!(pipe.finish(), Err(PeekError::Encode(_))));
    }

    #[test]
    fn invalid_settings_rejected_upfront() {
        let dest = temp_dest("bad");
        let settings = EncodeSettings {
            quality: 0,
            ..EncodeSettings::default()
        };
        let err = match Pipeline::new(settings, dest, None) {
            Ok(_) => panic!("expected InvalidQuality error"),
            Err(e) => e,
        };
        assert_eq!(err, PeekError::InvalidQuality(0));
    }
}
