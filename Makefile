.PHONY: all build install install-extension clean pack

EXTENSION_UUID = peek@Mehdi-Bahlaoui
EXTENSION_DIR = extension
CARGO_DIR = main
BIN = $(CARGO_DIR)/target/release/peek-rs

all: install

build:
	@echo "🔨 Building peek-rs (release)…"
	@cargo build --release --manifest-path $(CARGO_DIR)/Cargo.toml

install: build
	@mkdir -p ~/.local/bin
	@install -m 755 $(BIN) ~/.local/bin/peek-rs
	@$(MAKE) install-extension

install-extension:
	@echo "🧩 Installing extension…"
	@rm -rf ~/.local/share/gnome-shell/extensions/$(EXTENSION_UUID)
	@mkdir -p ~/.local/share/gnome-shell/extensions/$(EXTENSION_UUID)
	@cp -r $(EXTENSION_DIR)/* ~/.local/share/gnome-shell/extensions/$(EXTENSION_UUID)/
	@glib-compile-schemas ~/.local/share/gnome-shell/extensions/$(EXTENSION_UUID)/schemas/
	@echo "✅ Installed. Restart GNOME Shell (log out/in on Wayland), then:"
	@echo "   gnome-extensions enable $(EXTENSION_UUID)"

# packaging for extensions.gnome.org (binary is installed separately by
# scripts/install.sh; the zip contains only the shell extension)
pack:
	@echo "📦 Creating extension package…"
	@mkdir -p dist
	@rm -f $(EXTENSION_DIR)/schemas/gschemas.compiled
	@glib-compile-schemas $(EXTENSION_DIR)/schemas/
	@cd $(EXTENSION_DIR) && zip -r ../dist/$(EXTENSION_UUID).zip * -x "schemas/gschemas.compiled"
	@echo "✅ Package created: dist/$(EXTENSION_UUID).zip"

clean:
	@rm -rf $(EXTENSION_DIR)/schemas/gschemas.compiled dist
	@echo "✅ Clean complete."
