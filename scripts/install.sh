#!/bin/bash
set -e

REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
EXTENSION_UUID="peek@Mehdi-Bahlaoui"
EXTENSION_SRC="$REPO_DIR/extension"
EXTENSION_DST="$HOME/.local/share/gnome-shell/extensions/$EXTENSION_UUID"
BIN_DST="$HOME/.local/bin"

echo "🔨 Building peek-rs (release)…"
cargo build --release --manifest-path "$REPO_DIR/main/Cargo.toml"

echo "📦 Installing binary to $BIN_DST/peek-rs…"
mkdir -p "$BIN_DST"
install -m 755 "$REPO_DIR/main/target/release/peek-rs" "$BIN_DST/peek-rs"

echo "🧩 Installing GNOME extension…"
rm -rf "$EXTENSION_DST"
mkdir -p "$EXTENSION_DST"
cp -r "$EXTENSION_SRC"/* "$EXTENSION_DST/"

if command -v glib-compile-schemas >/dev/null; then
    glib-compile-schemas "$EXTENSION_DST/schemas/"
else
    echo "⚠️ glib-compile-schemas not found. Install libglib2.0-bin."
fi

echo "✅ Peek installed."
if gnome-extensions enable "$EXTENSION_UUID" 2>/dev/null; then
    echo "✅ Extension enabled. Press Ctrl+Shift+P to record."
else
    echo "🔄 Restart GNOME Shell (log out and back in on Wayland), then enable it:"
    echo "   gnome-extensions enable $EXTENSION_UUID"
    echo "⌨️  Then press Ctrl+Shift+P to record."
fi
