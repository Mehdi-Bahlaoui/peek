// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Peek Recorder — GNOME Shell launcher extension.
//
// The recorder itself is a GTK4/PipeWire app (`peek-rs`); this extension
// only wires a global shortcut to it. GTK's single-instance support
// means pressing the shortcut while Peek is already running just brings
// the existing window to the front.

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';

import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

const KEYBINDING = 'launch';
const BINARY_NAME = 'peek-rs';

export default class PeekExtension extends Extension {
    enable() {
        this._settings = this.getSettings();
        Main.wm.addKeybinding(
            KEYBINDING,
            this._settings,
            Meta.KeyBindingFlags.IGNORE_AUTOREPEAT,
            Shell.ActionMode.NORMAL | Shell.ActionMode.OVERVIEW,
            () => this._launch());
        console.log('[Peek] extension enabled');
    }

    disable() {
        Main.wm.removeKeybinding(KEYBINDING);
        this._settings = null;
        console.log('[Peek] extension disabled');
    }

    _launch() {
        const binary = this._resolveBinary();
        if (!binary) {
            Main.notifyError(
                'Peek not found',
                `Install it with scripts/install.sh from the Peek repository (${BINARY_NAME} is not on PATH or in ~/.local/bin).`);
            return;
        }

        try {
            // Fire and forget: the app owns its window and quits by
            // itself once a recording is saved.
            Gio.Subprocess.new([binary], Gio.SubprocessFlags.NONE);
        } catch (e) {
            Main.notifyError('Peek failed to launch', e.message);
        }
    }

    _resolveBinary() {
        const candidates = [];

        const configured = this._settings.get_string('binary').trim();
        if (configured)
            candidates.push(configured);

        candidates.push(GLib.build_filenamev([GLib.get_home_dir(), '.local', 'bin', BINARY_NAME]));
        candidates.push(GLib.build_filenamev(['/usr/local/bin', BINARY_NAME]));

        const fromPath = GLib.find_program_in_path(BINARY_NAME);
        if (fromPath)
            candidates.push(fromPath);

        for (const path of candidates) {
            if (GLib.file_test(path, GLib.FileTest.IS_EXECUTABLE))
                return path;
        }
        return null;
    }
}
