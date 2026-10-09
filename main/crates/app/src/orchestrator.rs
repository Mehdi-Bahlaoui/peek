//! Event loop: UI events in, snapshots out, one recording at a time.
//!
//! Recording data path (all fallible steps surface as `Failed` state):
//! portal dialog -> PipeWire frames -> locate our marker in live stream
//! frames -> crop -> encode worker thread -> `.gif` on disk. The encode
//! worker owns the `Pipeline` on a plain thread; the async capture loop
//! only forwards frames, so a slow GIF encode can never stall frame
//! pickup (stale frames drop at appsink).

use std::path::PathBuf;
use std::sync::mpsc as stdmpsc;
use std::time::Duration;

use directories::UserDirs;
use gtk4::glib;
use peek_capture::{
    crop_rgba, find_frame_rgba, start_monitor_cast, CastSession, MonitorStream, PipeWireSource,
};
use peek_core::{AppConfig, LogicalRect, PeekError, PhysicalRect, VideoFrame};
use peek_encode::{estimate, warn, EncodeResult, Pipeline};
use peek_ui::{UiEvent, UiSnapshot};
use tokio::sync::{mpsc, oneshot};

use crate::state::{dest_path, encode_settings, AppState};

const CONFIG_DIR: &str = "peek-rs";
const CONFIG_FILE: &str = "config.toml";

fn config_path() -> PathBuf {
    glib::user_config_dir().join(CONFIG_DIR).join(CONFIG_FILE)
}

/// Load persisted settings, or defaults on first run / bad file.
pub fn load_config() -> AppConfig {
    let path = config_path();
    let Ok(text) = std::fs::read_to_string(&path) else {
        return AppConfig::default();
    };
    AppConfig::from_toml_str(&text).unwrap_or_default()
}

/// Best-effort persist (failures are swallowed: settings must never
/// break a recording).
pub fn save_config(config: &AppConfig) {
    let path = config_path();
    if let Ok(text) = config.to_toml_string() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, text);
    }
}

fn video_dir() -> Option<PathBuf> {
    let dirs = UserDirs::new()?;
    Some(
        dirs.video_dir()
            .map(|d| d.join("Peek"))
            .unwrap_or_else(|| dirs.home_dir().join("Videos").join("Peek")),
    )
}

fn snap(state: &AppState, tx: &mpsc::UnboundedSender<UiSnapshot>) {
    let _ = tx.send(state.snapshot());
}

/// Ask the GTK application to shut down. `g_application_quit` is safe
/// from any thread: the main loop returns, the window closes, and the
/// process ends. No-op outside a running application (unit tests).
fn quit_app() {
    use gtk4::gio::prelude::ApplicationExt;
    if let Some(app) = gtk4::gio::Application::default() {
        app.quit();
    }
}

/// Main loop. Returns when the event channel closes or after a
/// successful save (the app's whole job is one recording).
pub async fn run(
    mut events: mpsc::UnboundedReceiver<UiEvent>,
    snapshots: mpsc::UnboundedSender<UiSnapshot>,
    mut config: AppConfig,
) {
    let mut state = AppState::Selecting;
    snap(&state, &snapshots);

    while let Some(event) = events.recv().await {
        match event {
            UiEvent::RecordPressed {
                view_w,
                view_h,
                mon,
            } => {
                if !matches!(
                    state,
                    AppState::Selecting | AppState::Done | AppState::Failed
                ) {
                    continue;
                }
                if let Ok(size) = LogicalRect::new(0, 0, view_w.max(1), view_h.max(1)) {
                    config.last_rect = Some(size);
                    save_config(&config);
                }
                let saved =
                    match run_recording(view_w, view_h, mon, &mut config, &mut events, &snapshots)
                        .await
                    {
                        Ok(path) => {
                            eprintln!("[peek] saved {}", path.display());
                            state = AppState::Done;
                            true
                        }
                        Err(reason) => {
                            eprintln!("[peek] FAILED: {reason}");
                            state = AppState::Failed;
                            false
                        }
                    };
                snap(&state, &snapshots);
                if saved {
                    // A finished recording is the whole job: after the
                    // GIF is on disk there is nothing left to do, so
                    // shut down instead of idling. On failure the window
                    // stays open so the user can retry.
                    quit_app();
                    return;
                }
            }
            UiEvent::StopPressed => {
                // Only meaningful inside run_recording's select loop.
            }
            UiEvent::FpsSelected(fps) => {
                config.fps = fps;
                save_config(&config);
            }
            UiEvent::QualitySelected(quality) => {
                if (1..=100).contains(&quality) {
                    config.quality = quality;
                    save_config(&config);
                }
            }
        }
    }
}

/// One recording: negotiate the portal, locate the viewfinder in live
/// stream frames, capture it, encode the GIF. Listens for `StopPressed`
/// while capturing. Returns the output path.
async fn run_recording(
    view_w: u32,
    view_h: u32,
    mon: Option<(u32, u32)>,
    config: &mut AppConfig,
    events: &mut mpsc::UnboundedReceiver<UiEvent>,
    snapshots: &mpsc::UnboundedSender<UiSnapshot>,
) -> Result<PathBuf, PeekError> {
    if view_w < 16 || view_h < 16 {
        return Err(PeekError::Capture(
            "viewfinder too small; resize the window larger".into(),
        ));
    }
    let (mon_w, mon_h) =
        mon.ok_or_else(|| PeekError::Capture("could not determine monitor geometry".into()))?;

    // 1. Portal: restore the saved monitor choice silently, or pop
    // the system picker on first use. The marker stays visible
    // throughout: detection runs on the first live stream frames, so
    // there is no screenshot — no shutter flash and no shutter sound.
    let outcome = start_monitor_cast(config.restore_token.as_deref()).await?;
    if outcome.restore_token != config.restore_token {
        config.restore_token = outcome.restore_token;
        save_config(config);
    }
    let mut cast = outcome.session;
    let stream = cast
        .streams
        .first()
        .copied()
        .ok_or_else(|| PeekError::Capture("portal returned no streams".into()))?;
    eprintln!("[peek] portal stream node {}", stream.node_id);

    // The session is closed on every path — normal end (inside
    // `capture_and_encode`, as soon as capture stops) and early errors
    // (fallback below). Left open it keeps GNOME's screen-share
    // indicator up and the PipeWire stream live.
    let restored = config.restore_token.is_some();
    let result = capture_and_encode(
        &mut cast, stream, view_w, view_h, mon_w, mon_h, config, events, snapshots,
    )
    .await;
    if let Err(e) = cast.close().await {
        eprintln!("[peek] portal close failed: {e}");
    }
    // A restored pick that no longer matches (window moved to another
    // monitor, monitor layout changed) would otherwise fail on every
    // attempt: drop the token so the next Record pops the picker again.
    if result.is_err() && restored {
        config.restore_token = None;
        save_config(config);
        eprintln!("[peek] saved monitor choice cleared; next Record will ask again");
    }
    result
}

/// Capture + encode while `cast` is alive. Split out of
/// [`run_recording`] so the portal session is closed exactly once, on
/// all paths.
#[allow(clippy::too_many_arguments)]
async fn capture_and_encode(
    cast: &mut CastSession,
    stream: MonitorStream,
    view_w: u32,
    view_h: u32,
    mon_w: u32,
    mon_h: u32,
    config: &AppConfig,
    events: &mut mpsc::UnboundedReceiver<UiEvent>,
    snapshots: &mpsc::UnboundedSender<UiSnapshot>,
) -> Result<PathBuf, PeekError> {
    // 2. PipeWire frames at full monitor resolution.
    let mut source = PipeWireSource::start(cast.fd(), stream.node_id).await?;
    eprintln!("[peek] stream frames {}x{}", source.width, source.height);
    let scale = f64::from(source.width) / f64::from(mon_w);
    let scale_h = f64::from(source.height) / f64::from(mon_h);
    if (scale - scale_h).abs() > 0.05 * scale {
        return Err(PeekError::Capture(
            "display scaling looks inconsistent; cannot map area".into(),
        ));
    }
    let ew = ((f64::from(view_w) * scale).round() as u32).max(2);
    let eh = ((f64::from(view_h) * scale).round() as u32).max(2);

    // 3. Detect our marker. The startup frame is tried first: it is
    // always real content, while later frames only arrive when the
    // screen changes (damage-driven stream). Retries cover the rare
    // blank startup buffer once the user moves the mouse.
    // These frames are consumed for detection only and never encoded.
    eprintln!(
        "[peek] detect: view={}x{} mon={}x{} scale={:.3} expect={}x{}",
        view_w, view_h, mon_w, mon_h, scale, ew, eh
    );
    let mut startup = source.take_first_frame();
    let mut det = None;
    let mut last_err = PeekError::Capture("no stream frames arrived".into());
    let mut logged_first = false;
    let mut first_dump: Option<(u32, u32, Vec<u8>)> = None;
    for attempt in 0..61 {
        let frame = if attempt == 0 {
            startup.take()
        } else {
            source.pull_frame()?
        };
        if let Some(frame) = frame {
            if !logged_first {
                logged_first = true;
                eprintln!("[peek] first frame {}x{}", frame.width, frame.height);
                first_dump = Some((frame.width, frame.height, frame.rgba.clone()));
            }
            match find_frame_rgba(&frame.rgba, frame.width, frame.height, ew, eh) {
                Ok(d) => {
                    det = Some(d);
                    break;
                }
                Err(e) => last_err = e,
            }
        }
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    let det = det.ok_or_else(|| {
        if let Some((w, h, rgba)) = first_dump {
            dump_frame_for_debug(w, h, &rgba);
        }
        last_err
    })?;
    eprintln!(
        "[peek] marker at {}x{}+{}+{}",
        det.width, det.height, det.x, det.y
    );

    // 4. Recording state: marker hides, input shapes to the header.
    // The hide is applied on the GTK main loop and commits new window
    // content, which makes the compositor send a fresh (marker-free)
    // frame. Keep that frame: it becomes the GIF's first frame, so a
    // recording of a still screen is still a valid GIF.
    let state = AppState::Recording;
    snap(&state, snapshots);
    let mut marker_gone = false;
    let mut first_clean: Option<VideoFrame> = None;
    for _ in 0..80 {
        if let Some(frame) = source.pull_frame()? {
            if find_frame_rgba(&frame.rgba, frame.width, frame.height, ew, eh).is_err() {
                marker_gone = true;
                first_clean = Some(frame);
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    if !marker_gone {
        eprintln!("[peek] warning: viewfinder marker still visible in the stream");
    }

    // 5. The detected rect is already in stream pixels (detection ran on
    // the stream frames), so containment is a plain bounds check.
    let crop = fit_in_stream(&det, source.width, source.height)?;
    eprintln!(
        "[peek] cropping {}x{}+{}+{}",
        crop.width, crop.height, crop.x, crop.y
    );

    // 6. Size heads-up, framed per 10s so it reads sensibly in the log.
    let est = estimate(
        crop.width as u64 * crop.height as u64,
        config.fps,
        config.quality,
        10,
    );
    if let Some(warning) = warn(&est) {
        eprintln!("[peek] {warning}");
    }

    // 7. Encode worker thread owns the Pipeline.
    let dest = dest_path(video_dir());
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| PeekError::Io(e.to_string()))?;
    }
    let settings = encode_settings(config);
    let (frame_tx, frame_rx) = stdmpsc::channel::<Option<VideoFrame>>();
    let (done_tx, done_rx) = oneshot::channel::<Result<EncodeResult, PeekError>>();
    let worker_dest = dest.clone();
    std::thread::spawn(move || {
        let mut pipe = match Pipeline::new(settings, worker_dest, None) {
            Ok(pipe) => pipe,
            Err(e) => {
                let _ = done_tx.send(Err(e));
                return;
            }
        };
        while let Ok(msg) = frame_rx.recv() {
            match msg {
                Some(frame) => {
                    if let Err(e) = pipe.push_frame(&frame) {
                        let _ = done_tx.send(Err(e));
                        return;
                    }
                }
                None => break,
            }
        }
        let _ = done_tx.send(pipe.finish());
    });

    // 8. Capture loop at fps cadence until Stop.
    let mut interval = tokio::time::interval(config.fps.frame_interval());
    // First tick fires immediately; skip it so timing starts clean.
    interval.tick().await;
    let mut frames = 0usize;

    // The marker-free frame captured right after the hide opens the
    // GIF; without it a still screen would encode zero frames (the
    // stream delivers nothing while nothing moves).
    if let Some(frame) = first_clean.take() {
        let bytes = crop_rgba(&frame.rgba, frame.width, frame.height, &crop)?;
        let cropped = VideoFrame::new(crop.width, crop.height, frame.pts, bytes)?;
        frames += 1;
        if frame_tx.send(Some(cropped)).is_err() {
            return Err(PeekError::Encode("encode worker died".into()));
        }
    }

    let capture_result: Result<(), PeekError> = loop {
        tokio::select! {
            _ = interval.tick() => {
                if let Some(frame) = source.pull_frame()? {
                    let bytes = crop_rgba(&frame.rgba, frame.width, frame.height, &crop)?;
                    let cropped =
                        VideoFrame::new(crop.width, crop.height, frame.pts, bytes)?;
                    frames += 1;
                    if frames.is_multiple_of(30) {
                        eprintln!("[peek] {frames} frames captured…");
                    }
                    if frame_tx.send(Some(cropped)).is_err() {
                        break Err(PeekError::Encode("encode worker died".into()));
                    }
                }
            }
            event = events.recv() => {
                match event {
                    Some(UiEvent::StopPressed) | None => break Ok(()),
                    _ => {}
                }
            }
        }
    };
    capture_result?;
    drop(frame_tx);
    source.stop();
    // Release the portal session (GNOME's screen-share indicator) before
    // the potentially slow encode. Idempotent; the caller re-closes as
    // the fallback for early-error paths.
    if let Err(e) = cast.close().await {
        eprintln!("[peek] portal close failed: {e}");
    }

    // Encode runs on its worker thread; surface it so the UI disables
    // Stop while the GIF is being written.
    let encoding = AppState::Encoding;
    snap(&encoding, snapshots);

    let result = done_rx
        .await
        .map_err(|_| PeekError::Encode("encode worker vanished".into()))??;
    eprintln!(
        "[peek] encoded {} frames to {} ({} bytes)",
        result.frames,
        result.path.display(),
        result.bytes
    );
    if result.frames == 0 {
        return Err(PeekError::Encode("no frames captured".into()));
    }
    Ok(result.path)
}

/// Save the first stream frame as raw RGBA plus a color census, so a
/// detection failure can be diagnosed offline instead of guessed at.
fn dump_frame_for_debug(w: u32, h: u32, rgba: &[u8]) {
    let mut violet = 0u64;
    let mut blueish = 0u64;
    let (chunks, _) = rgba.as_chunks::<4>();
    for p in chunks {
        if (106..=126).contains(&p[0]) && (86..=106).contains(&p[1]) && (202..=222).contains(&p[2])
        {
            violet += 1;
        }
        if p[0] < 96 && p[1] < 96 && p[2] > 64 {
            blueish += 1;
        }
    }
    let path = format!("/tmp/peek-rs-frame-{w}x{h}.rgba");
    let _ = std::fs::write(&path, rgba);
    eprintln!("[peek] detection failed: violet={violet} blueish={blueish} in {w}x{h}");
    eprintln!("[peek] raw frame saved to {path} (view: convert -size {w}x{h} -depth 8 RGBA:{path} frame.png)");
}

/// The detected rect is already in stream pixels (detection ran on the
/// stream frames), so this is a plain containment check against the
/// frame. The portal's stream position is in desktop logical
/// coordinates and must not be subtracted here.
fn fit_in_stream(
    area: &PhysicalRect,
    frame_w: u32,
    frame_h: u32,
) -> Result<PhysicalRect, PeekError> {
    if area.x < 0
        || area.y < 0
        || (area.x as u32).saturating_add(area.width) > frame_w
        || (area.y as u32).saturating_add(area.height) > frame_h
    {
        return Err(PeekError::Capture(
            "detected area is outside the picked monitor — pick the monitor under the window"
                .into(),
        ));
    }
    PhysicalRect::new(area.x, area.y, area.width, area.height)
        .map_err(|_| PeekError::Capture("detected rect degenerate".into()))
}
