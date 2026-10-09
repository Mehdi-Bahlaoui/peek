//! `peek-rs` binary. GTK owns the main thread; a process-lifetime Tokio
//! runtime (kept in a `OnceLock`, never shut down before exit) powers
//! the portal, capture, and orchestration tasks.

use std::sync::OnceLock;

use gtk4::prelude::*;
use peek_app::{load_config, run};

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

fn main() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("start tokio runtime");
    let _ = RUNTIME.set(runtime);
    let handle = RUNTIME.get().expect("runtime set").handle().clone();

    let app = gtk4::Application::new(Some("dev.peek_rs.app"), Default::default());
    app.connect_activate(move |gtk_app| {
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
        let config = load_config();
        let ui = peek_ui::build_ui(gtk_app, event_tx, &config);
        handle.spawn(run(event_rx, ui.snapshot_tx, config));
    });
    app.run();
}
