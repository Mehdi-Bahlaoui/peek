# Peek — Wayland GIF recorder for GNOME

A from-scratch Rust successor to Peek (`phw/peek`, deprecated upstream):
a one-window GIF recorder for GNOME Wayland sessions. This repository
bundles it as a GNOME Shell extension that launches the recorder with
**Ctrl+Shift+P**.

- **One-window model** — the window *is* the frame: move it by the
  header, resize it by the edges, press Record. No overlay, no selection
  step, no screenshot (no shutter flash and no shutter sound).
- **Any area, any monitor** — the viewfinder is located in the live
  PipeWire stream itself, so the crop is pixel-exact at any scale
  (fractional scaling included).
- **GIF-only, 10/15/30 fps**, quality 50/80/100% on a snapping slider.
- **Seamless recording** — the marker disappears before capture starts,
  clicks pass through to the apps you record, and the window hides while
  the GIF encodes. The app exits by itself once the file is saved.

## Requirements

- Ubuntu 26.04 (or any GNOME 45+ Wayland session)
- Rust toolchain (`cargo`)
- `libgtk-4-dev`, `libgstreamer1.0-dev`,
  `libgstreamer-plugins-base1.0-dev`, `libpipewire-0.3-dev`,
  `gstreamer1.0-pipewire`, `glib-compile-schemas` (`libglib2.0-bin`)

```sh
sudo apt install libgtk-4-dev libgstreamer1.0-dev \
    libgstreamer-plugins-base1.0-dev libpipewire-0.3-dev \
    gstreamer1.0-pipewire libglib2.0-bin
```

## Installation

```sh
# Build the recorder, install the binary to ~/.local/bin and the
# extension to ~/.local/share/gnome-shell/extensions
./scripts/install.sh
```

or with `make`:

```sh
make install
```

Then restart GNOME Shell (log out and back in on Wayland) and enable
the extension:

```sh
gnome-extensions enable peek@Mehdi-Bahlaoui
```

## Usage

1. Press **Ctrl+Shift+P** (or launch `peek-rs` from a terminal). Pressing
   the shortcut again while Peek is open brings the window to the front.
2. Move/resize the window over the area you want to record. The violet
   frame is the exact boundary.
3. Press **Record** (button or Enter), pick the monitor in the system
   dialog (only the first time — the choice is remembered).
4. Interact freely; clicks pass through everywhere except the header.
5. Press **Stop** (button or Escape). The window hides while the GIF
   encodes and the app closes itself once the file is saved to
   `~/Videos/Peek/Peek <date> <time>.gif`.

### Custom shortcut

The shortcut is stored in GSettings (default `<Control><Shift>p`):

```sh
dconf write /org/gnome/shell/extensions/peek/launch "['<Control><Shift>o']"
```

A non-standard binary location can be set with
`dconf write /org/gnome/shell/extensions/peek/binary "'/path/to/peek-rs'"`.

### Uninstall

```sh
rm -rf ~/.local/share/gnome-shell/extensions/peek@Mehdi-Bahlaoui
rm -f ~/.local/bin/peek-rs
```

## Repository layout

- `extension/` — the GNOME Shell extension (launcher + keybinding).
- `scripts/install.sh` — builds and installs binary + extension.
- `main/` — the recorder itself (Rust workspace), see
  [`main/README.md`](main/README.md) for internals.

## License

`gifski` is AGPL-3.0-or-later, so this project is distributed under the
**AGPL-3.0-or-later** as well.
