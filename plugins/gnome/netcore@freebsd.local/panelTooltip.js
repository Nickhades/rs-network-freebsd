// Tooltip for the netcore panel button.
//
// GNOME Shell's PanelMenu.Button has no tooltip API (unlike Gtk).  Use the
// same approach as the shell's own screenshot UI: a St.Label added to
// Main.uiGroup and positioned below the panel button.  The class owns the
// hover handler, the show delay and the label, and tears all of them down in
// destroy().

import Clutter from 'gi://Clutter';
import GLib from 'gi://GLib';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

const TOOLTIP_DELAY_MS = 350;

export class PanelTooltip {
    constructor(actor) {
        this._actor = actor;
        this._destroyed = false;
        this._timeoutId = 0;
        this._hoverId = 0;
        this._menuId = 0;

        this._label = new St.Label({
            style_class: 'dash-label netcore-tooltip',
            visible: false,
            reactive: false,
            can_focus: false,
        });
        Main.uiGroup.add_child(this._label);

        this._hoverId = actor.connect('notify::hover', () => {
            if (this._destroyed)
                return;
            if (actor.hover)
                this._scheduleShow();
            else
                this.hide();
        });

        this._menuId = actor.menu.connect('open-state-changed', (_menu, open) => {
            if (open)
                this.hide();
        });
    }

    setText(text) {
        if (this._destroyed)
            return;
        const value = String(text ?? '');
        if (this._label.text === value)
            return;
        this._label.text = value;
        if (this._label.visible)
            this._place();
    }

    _scheduleShow() {
        if (this._timeoutId || this._destroyed)
            return;
        this._timeoutId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, TOOLTIP_DELAY_MS, () => {
            this._timeoutId = 0;
            if (!this._destroyed && this._actor.hover)
                this._show();
            return GLib.SOURCE_REMOVE;
        });
    }

    _show() {
        if (this._destroyed)
            return;
        this._label.remove_all_transitions();
        this._label.opacity = 0;
        this._label.show();
        this._place();
        this._label.ease({
            opacity: 255,
            duration: 120,
            mode: Clutter.AnimationMode.EASE_OUT_QUAD,
        });
    }

    _place() {
        const extents = this._actor.get_transformed_extents();
        const [, naturalWidth] = this._label.get_preferred_width(-1);
        const [, naturalHeight] = this._label.get_preferred_height(naturalWidth);
        const stageWidth = global.stage.width;
        const centered = extents.get_x() + (extents.get_width() - naturalWidth) / 2;
        const x = Math.max(6, Math.min(Math.round(centered), stageWidth - naturalWidth - 6));
        const y = Math.round(extents.get_y() + extents.get_height() + 6);
        this._label.set_position(x, y);
    }

    hide() {
        if (this._destroyed)
            return;
        if (this._timeoutId) {
            GLib.source_remove(this._timeoutId);
            this._timeoutId = 0;
        }
        if (!this._label.visible)
            return;
        this._label.remove_all_transitions();
        this._label.ease({
            opacity: 0,
            duration: 100,
            mode: Clutter.AnimationMode.EASE_OUT_QUAD,
            onComplete: () => {
                if (!this._destroyed)
                    this._label.hide();
            },
        });
    }

    destroy() {
        if (this._destroyed)
            return;
        this._destroyed = true;

        if (this._timeoutId) {
            GLib.source_remove(this._timeoutId);
            this._timeoutId = 0;
        }
        if (this._hoverId) {
            this._actor.disconnect(this._hoverId);
            this._hoverId = 0;
        }
        if (this._menuId) {
            this._actor.menu.disconnect(this._menuId);
            this._menuId = 0;
        }

        this._label.destroy();
        this._label = null;
        this._actor = null;
    }
}
