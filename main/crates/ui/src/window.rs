//! Single Peek window: header chrome plus a transparent viewfinder.
//! The window IS the frame: drag it by the header, resize it by the
//! window edges (both free from the toolkit/compositor on Wayland), and
//! the violet marker painted around the viewfinder locates the area at
//! record time (see `capture::locate`). No fullscreen overlay, no second
//! window, no selection step.

use std::cell::Cell;
use std::rc::Rc;

use gtk4::gdk;
use gtk4::gdk::prelude::*;
use gtk4::prelude::*;
use peek_core::{AppConfig, Fps};
use tokio::sync::mpsc::UnboundedSender;

use crate::{UiEvent, UiSnapshot, UiState};

const MARK_THICK: f64 = 3.0;

/// Quality stops the slider snaps to, as percentages. The ends of the
/// slider are the extremes: 50% and 100%.
const QUALITY_STOPS: [u8; 3] = [50, 80, 100];

/// Index of the stop closest to an arbitrary stored quality (1..=100),
/// so hand-edited or older config values land on the slider sensibly.
fn nearest_stop(quality: u8) -> usize {
    QUALITY_STOPS
        .iter()
        .enumerate()
        .min_by_key(|(_, stop)| quality.abs_diff(**stop))
        .map(|(i, _)| i)
        .unwrap_or(0)
}

fn stop_label(quality: u8) -> String {
    format!("{quality}%")
}

pub struct PeekWindow {
    window: gtk4::ApplicationWindow,
    header: gtk4::HeaderBar,
    record: gtk4::Button,
    stop: gtk4::Button,
    view: gtk4::DrawingArea,
    frame_on: Rc<Cell<bool>>,
    /// Last state applied, so `present` can respect the hidden-while-
    /// encoding behavior.
    state: Cell<UiState>,
}

pub fn show_window(
    app: &gtk4::Application,
    events: &UnboundedSender<UiEvent>,
    initial: &AppConfig,
) -> PeekWindow {
    let window = gtk4::ApplicationWindow::new(app);
    // NOTE: no set_decorated(false) — in GTK4 that hides the titlebar
    // widget too. The custom HeaderBar below is our decoration: it keeps
    // drag-to-move, edge resize, and the close button working on Wayland.
    window.set_title(Some("Peek"));
    window.set_default_size(520, 420);
    window.add_css_class("peek-window");

    if let Some(display) = gdk::Display::default() {
        let css = gtk4::CssProvider::new();
        css.load_from_data(
            ".peek-window, .peek-view { background-color: transparent; background-image: none; }",
        );
        gtk4::style_context_add_provider_for_display(
            &display,
            &css,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
    window.connect_realize(|w| {
        use gtk4::prelude::NativeExt;
        if let Some(surface) = w.surface() {
            surface.set_opaque_region(Some(&gtk4::cairo::Region::create()));
        }
    });

    let header = gtk4::HeaderBar::new();
    header.set_title_widget(Some(&gtk4::Label::new(Some("Peek"))));

    let record = gtk4::Button::with_label("● Record");
    let stop = gtk4::Button::with_label("■ Stop");
    stop.set_sensitive(false);
    {
        let events = events.clone();
        stop.connect_clicked(move |_| {
            let _ = events.send(UiEvent::StopPressed);
        });
    }
    header.pack_start(&record);
    header.pack_start(&stop);

    let fps_combo = gtk4::ComboBoxText::new();
    for label in ["10", "15", "30"] {
        fps_combo.append_text(label);
    }
    let active: u32 = match initial.fps {
        Fps::Fps10 => 0,
        Fps::Fps15 => 1,
        Fps::Fps30 => 2,
    };
    fps_combo.set_active(Some(active));
    {
        let events = events.clone();
        fps_combo.connect_changed(move |combo| {
            if let Some(text) = combo.active_text() {
                if let Ok(v) = text.parse::<u8>() {
                    if let Ok(fps) = Fps::from_u8(v) {
                        let _ = events.send(UiEvent::FpsSelected(fps));
                    }
                }
            }
        });
    }
    header.pack_end(&fps_combo);

    // Quality: three stops only (50 / 80 / 100%), with the ends of the
    // slider as the extremes. `round_digits(0)` filters user input to
    // whole steps, so the handle snaps and never rests in between.
    let quality = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, 0.0, 2.0, 1.0);
    quality.set_round_digits(0);
    quality.set_increments(1.0, 1.0);
    quality.set_draw_value(false);
    quality.set_size_request(200, -1);
    quality.set_tooltip_text(Some("GIF quality"));
    for (i, _) in QUALITY_STOPS.iter().enumerate() {
        quality.add_mark(i as f64, gtk4::PositionType::Bottom, None);
    }
    let start = nearest_stop(initial.quality);
    quality.set_value(start as f64);

    let quality_label = gtk4::Label::new(Some(&stop_label(QUALITY_STOPS[start])));
    quality_label.set_width_chars(4);
    quality_label.add_css_class("dim-label");

    // A stored quality that is not one of the stops (hand-edited config,
    // older version) is normalized so the encoder and slider agree from
    // the first recording on.
    if QUALITY_STOPS[start] != initial.quality {
        let _ = events.send(UiEvent::QualitySelected(QUALITY_STOPS[start]));
    }
    {
        let events = events.clone();
        let label = quality_label.clone();
        quality.connect_value_changed(move |scale| {
            let idx = (scale.value().round() as usize).min(QUALITY_STOPS.len() - 1);
            label.set_text(&stop_label(QUALITY_STOPS[idx]));
            let _ = events.send(UiEvent::QualitySelected(QUALITY_STOPS[idx]));
        });
    }
    header.pack_end(&quality_label);
    header.pack_end(&quality);
    window.set_titlebar(Some(&header));

    let frame_on = Rc::new(Cell::new(true));
    let view = gtk4::DrawingArea::new();
    view.add_css_class("peek-view");
    view.set_hexpand(true);
    view.set_vexpand(true);
    view.set_size_request(240, 160);
    {
        let frame_on = frame_on.clone();
        view.set_draw_func(move |_, cr, w, h| {
            if !frame_on.get() {
                return;
            }
            // Violet marker (Ubuntu accent 116, 96, 212). Fractions so
            // the painted bytes land exactly (Cairo quantizes to bytes).
            cr.set_source_rgba(116.0 / 255.0, 96.0 / 255.0, 212.0 / 255.0, 1.0);
            let (w, h) = (f64::from(w), f64::from(h));
            let t = MARK_THICK;
            for (x, y, ww, hh) in [
                (0.0, 0.0, w, t),
                (0.0, h - t, w, t),
                (0.0, 0.0, t, h),
                (w - t, 0.0, t, h),
            ] {
                cr.rectangle(x, y, ww, hh);
                let _ = cr.fill();
            }
        });
    }
    window.set_child(Some(&view));

    // Real record wiring (needs `view` + `window`).
    {
        let events = events.clone();
        let view = view.clone();
        let window = window.clone();
        record.connect_clicked(move |_| {
            emit_record(&view, &window, &events);
        });
    }

    // Keyboard: Return records, Escape stops.
    let key = gtk4::EventControllerKey::new();
    {
        let events = events.clone();
        let view = view.clone();
        let window = window.clone();
        key.connect_key_pressed(move |_, keyval, _, _| match keyval {
            gdk::Key::Return | gdk::Key::KP_Enter => {
                emit_record(&view, &window, &events);
                gtk4::glib::Propagation::Stop
            }
            gdk::Key::Escape => {
                let _ = events.send(UiEvent::StopPressed);
                gtk4::glib::Propagation::Stop
            }
            _ => gtk4::glib::Propagation::Proceed,
        });
    }
    window.add_controller(key);

    window.present();
    PeekWindow {
        window,
        header,
        record,
        stop,
        view,
        frame_on,
        state: Cell::new(UiState::Selecting),
    }
}

/// Read the live viewfinder + monitor geometry on the GTK thread and
/// hand them to the recorder. Logical pixels throughout.
fn emit_record(
    view: &gtk4::DrawingArea,
    window: &gtk4::ApplicationWindow,
    events: &UnboundedSender<UiEvent>,
) {
    let view_w = view.allocated_width().max(0) as u32;
    let view_h = view.allocated_height().max(0) as u32;
    let mon = monitor_size(window);
    let _ = events.send(UiEvent::RecordPressed {
        view_w,
        view_h,
        mon,
    });
}

fn surface_of(window: &gtk4::ApplicationWindow) -> Option<gdk::Surface> {
    use gtk4::prelude::NativeExt;
    window.surface()
}

fn monitor_size(window: &gtk4::ApplicationWindow) -> Option<(u32, u32)> {
    let surface = surface_of(window)?;
    let display = gdk::Display::default()?;
    let monitor = display.monitor_at_surface(&surface)?;
    let geo = monitor.geometry();
    (geo.width() > 0 && geo.height() > 0).then_some((geo.width() as u32, geo.height() as u32))
}

impl PeekWindow {
    pub fn apply_snapshot(&self, snap: &UiSnapshot) {
        self.state.set(snap.state);
        let recording = matches!(snap.state, UiState::Recording);
        // Marker off while recording: the frame must not end up in the GIF.
        self.frame_on.set(!recording);
        self.view.queue_draw();
        // Stop pressed: hide the window while the GIF encodes so it
        // cannot be closed by accident and does not distract. It comes
        // back only when encoding fails (retry is still possible); on
        // success the app quits by itself.
        match snap.state {
            UiState::Encoding => self.window.set_visible(false),
            UiState::Failed => self.window.set_visible(true),
            _ => {}
        }
        // Click-through everywhere except the header strip, so the user
        // can interact with the recorded apps while Stop stays clickable.
        // Out-of-surface region parts are ignored by compositors.
        if let Some(surface) = surface_of(&self.window) {
            if recording {
                let strip = self.header.allocated_height().max(8);
                let region = gtk4::cairo::Region::create_rectangle(
                    &gtk4::cairo::RectangleInt::new(0, 0, 100_000, strip),
                );
                surface.set_input_region(Some(&region));
            } else {
                surface.set_input_region(None);
            }
        }
        match snap.state {
            UiState::Selecting | UiState::Armed | UiState::Done | UiState::Failed => {
                // Record stays live after a finished or failed take:
                // retry must never require restarting the app.
                self.record.set_sensitive(true);
                self.stop.set_sensitive(false);
            }
            UiState::Recording => {
                self.record.set_sensitive(false);
                self.stop.set_sensitive(true);
            }
            UiState::Encoding => {
                self.record.set_sensitive(false);
                self.stop.set_sensitive(false);
            }
        }
    }

    /// Bring the window back to the front (the launcher shortcut was
    /// pressed while Peek is already running). No-op while encoding:
    /// the window is deliberately hidden then.
    pub fn present(&self) {
        if !matches!(self.state.get(), UiState::Encoding) {
            self.window.present();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_stop_clamps_to_the_extremes() {
        // Anything at or below 50% lands on the low extreme; anything
        // above the 80/100 midpoint lands on 100%.
        assert_eq!(QUALITY_STOPS[nearest_stop(1)], 50);
        assert_eq!(QUALITY_STOPS[nearest_stop(45)], 50);
        assert_eq!(QUALITY_STOPS[nearest_stop(50)], 50);
        assert_eq!(QUALITY_STOPS[nearest_stop(100)], 100);
        assert_eq!(QUALITY_STOPS[nearest_stop(99)], 100);
    }

    #[test]
    fn nearest_stop_picks_closest_with_lower_tie_break() {
        assert_eq!(QUALITY_STOPS[nearest_stop(66)], 80);
        assert_eq!(QUALITY_STOPS[nearest_stop(80)], 80);
        assert_eq!(QUALITY_STOPS[nearest_stop(91)], 100);
        // Exact midpoints stay on the lower stop.
        assert_eq!(QUALITY_STOPS[nearest_stop(65)], 50);
        assert_eq!(QUALITY_STOPS[nearest_stop(90)], 80);
    }

    #[test]
    fn stop_labels_are_percentages() {
        assert_eq!(stop_label(50), "50%");
        assert_eq!(stop_label(100), "100%");
    }
}
