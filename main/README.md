# peek-rs — Wayland-native GIF recorder for Ubuntu 26.04

A from-scratch Rust successor to Peek (`phw/peek`, deprecated upstream),
built for GNOME Wayland-only sessions. GIF-only, 10/15/30 fps, quality
50/80/100%.

## The one-window model

There is exactly one window, and it IS the frame: move it by the header,
resize it by the window edges, press Record. No fullscreen overlay, no
second window, no selection step.

Wayland never tells a client its own position, so at Record time the app
first live PipeWire frames are scanned for the marker frame painted
around the viewfinder (solid violet bars at the widget bounds). The
match is validated against the expected device-pixel size and fails
loudly instead of mis-cropping — and since no screenshot is taken,
recording starts with no shutter flash. During recording the marker hides, input shapes to the
header strip (click-through everywhere else, Stop stays clickable), and
the picked monitor stream is cropped to the detected rect.

## Layout

- `crates/core` — shared types: rects, fps, config, frames, errors. No system deps.
- `crates/capture` — ScreenCast portal + PipeWire frames (GStreamer),
  self-localization in stream frames, rect crop.
- `crates/encode` — `gifski` GIF pipeline + pre-record size estimator.
- `crates/ui` — the single GTK4 window (header chrome + viewfinder).
- `crates/app` — state machine, capture loop, encode worker, `peek-rs` binary.

## Prerequisites (Ubuntu 26.04)

```sh
sudo apt install libgtk-4-dev libgstreamer1.0-dev \
    libgstreamer-plugins-base1.0-dev libpipewire-0.3-dev
gst-inspect-1.0 pipewiresrc  # must exist; provides the portal->GStreamer bridge
```

## Build & test

```sh
cargo test --workspace   # pure-logic tests; portal/UI paths need a live session
cargo run -p peek-app    # launches the recorder
```

## Use

1. Move/resize the Peek window over the area. The violet frame is the
   exact recording boundary.
2. Press Record (button or Enter), pick the monitor in the system dialog.
3. Interact freely — clicks pass through except over the header.
4. Press Stop. Output lands in `~/Videos/Peek/Peek <date> <time>.gif`,
   and the app closes itself once the file is saved.

Settings persist in `~/.config/peek-rs/config.toml`. Keep other windows
clear of the violet frame at Record press so detection locks on cleanly.

## Notes

- Single-monitor setups just work. Multi-monitor: pick the monitor under
  the window in the portal dialog; the detected area must lie inside the
  picked stream.
- `gifski` is AGPL-3.0-or-later: distribute this project under a
  compatible license (AGPL-3.0-or-later recommended).
