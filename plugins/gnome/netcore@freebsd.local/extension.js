// netcore GNOME Shell extension entry point.

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

import {NetcoreIndicator} from './indicator.js';

export default class NetcoreExtension extends Extension {
    enable() {
        this._indicator = new NetcoreIndicator();
        Main.panel.addToStatusArea(this.uuid, this._indicator);
    }

    disable() {
        if (this._indicator) {
            this._indicator.destroy();
            this._indicator = null;
        }
    }
}
