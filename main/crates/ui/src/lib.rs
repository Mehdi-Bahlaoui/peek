//! Single-window GTK4 interface: one Peek window whose viewfinder is
//! the recording area. Everything GTK lives on the main thread; user
//! actions leave over a Tokio unbounded channel (sync send, never
//! blocks); state snapshots arrive the same way and are applied on the
//! main loop.

pub mod window;

use peek_core::{AppConfig, Fps};
use tokio::sync::mpsc::UnboundedSender;

/// Actions flowing ui -> app. Sizes are logical pixels, read live on
/// the GTK thread at click time.
#[derive(Debug, Clone)]
pub enum UiEvent {
    /// Viewfinder size plus the monitor's logical size (for scale math).
    /// `mon` is `None` when GDK cannot name the monitor; recording then
    /// fails with a clear error instead of a wrong crop.
    RecordPressed {
        view_w: u32,
        view_h: u32,
        mon: Option<(u32, u32)>,
    },
    StopPressed,
    FpsSelected(Fps),
    QualitySelected(u8),
}

/// Simplified app state for display purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiState {
    Selecting,
    Armed,
    Recording,
    Encoding,
    Done,
    Failed,
}

/// State flowing app -> ui. Deliberately minimal: human-readable text
/// lives in the terminal log (`[peek] …`), never in the window.
#[derive(Debug, Clone)]
pub struct UiSnapshot {
    pub state: UiState,
}

pub struct UiHandle {
    pub snapshot_tx: UnboundedSender<UiSnapshot>,
}

/// Build the window and wire snapshot delivery. Call from the GTK
/// `activate` handler (main thread owns the default main context).
pub fn build_ui(
    app: &gtk4::Application,
    events: UnboundedSender<UiEvent>,
    initial: &AppConfig,
) -> UiHandle {
    let peek = window::show_window(app, &events, initial);
    // Snapshots arrive on Tokio channels but must be applied on the GTK
    // thread: pump them from a main-loop future (the Tokio receiver is
    // pollable by any executor).
    let (snapshot_tx, mut snapshot_rx) = tokio::sync::mpsc::unbounded_channel::<UiSnapshot>();
    gtk4::glib::spawn_future_local(async move {
        while let Some(snap) = snapshot_rx.recv().await {
            peek.apply_snapshot(&snap);
        }
    });
    UiHandle { snapshot_tx }
}
