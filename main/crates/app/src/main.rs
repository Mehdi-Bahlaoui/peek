//! `peek-rs` binary. GTK owns the main thread; a process-lifetime Tokio
//! runtime (kept in a `OnceLock`, never shut down before exit) powers
//! the portal, capture, and orchestration tasks.

use std::cell::RefCell;
use std::rc::Rc;
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
    // Single instance: when the launcher shortcut is pressed while Peek
    // is already running, GTK activates this process instead of starting
    // a second one. Re-present the existing window instead of building
    // another.
    let ui: Rc<RefCell<Option<peek_ui::UiHandle>>> = Rc::new(RefCell::new(None));
    app.connect_activate(move |gtk_app| {
        if let Some(existing) = ui.borrow().as_ref() {
            existing.present();
            return;
        }
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
        let config = load_config();
        let ui_handle = peek_ui::build_ui(gtk_app, event_tx, &config);
        handle.spawn(run(event_rx, ui_handle.snapshot_tx.clone(), config));
        *ui.borrow_mut() = Some(ui_handle);
    });
    app.run();
}
