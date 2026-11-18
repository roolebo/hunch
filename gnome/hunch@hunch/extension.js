// hunch's GNOME Shell extension. On Wayland only the Shell sees its
// windows and types into them, so it serves GNOME's desktop to
// hunch-host as the wm gnome:, as skills/hunch/HOST.md says; and it's the
// seat's tray, in the top bar, as Hunch's menu is on a Mac.

import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import Shell from 'gi://Shell';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

import {LOCAL, LOCALHOST, hostName, hunchHost, micAllow} from './hunch.js';

// How long a line of hunch-host that ended waits before it runs again,
// as when the service restarts
const RERUN_SECONDS = 3;
// How long the window has to take the text from the clipboard before the
// clipboard, and the focus, get back what they held
const RESTORE_MS = 300;
// How long a window has to take the focus
const FOCUS_MS = 1000;
// How long an error stays on the pill
const ERROR_SECONDS = 4;
// WM.md's exit statuses
const GONE = 4;
const CANCELLED = 5;

/// Runs argv for as long as the extension is on, again when it ends, and
/// hands each line it prints to onLine; write sends it a line
class Lines {
    constructor(argv, onLine) {
        this._argv = argv;
        this._onLine = onLine;
        this._run();
    }

    _run() {
        this._cancel = new Gio.Cancellable();
        try {
            this._proc = Gio.Subprocess.new(this._argv,
                Gio.SubprocessFlags.STDIN_PIPE | Gio.SubprocessFlags.STDOUT_PIPE);
        } catch (e) {
            logError(e, `hunch: can't run ${this._argv.join(' ')}`);
            this._rerun();
            return;
        }
        this._stdout = new Gio.DataInputStream({
            base_stream: this._proc.get_stdout_pipe(),
            close_base_stream: true,
        });
        this._read();
        this._proc.wait_async(this._cancel, () => this._rerun());
    }

    _read() {
        this._stdout.read_line_async(GLib.PRIORITY_DEFAULT, this._cancel, (stream, result) => {
            let line;
            try {
                [line] = stream.read_line_finish_utf8(result);
            } catch (e) {
                return;
            }
            if (line === null)
                return;
            try {
                this._onLine(JSON.parse(line));
            } catch (e) {
                logError(e, `hunch: ${this._argv.join(' ')} said ${line}`);
            }
            this._read();
        });
    }

    _rerun() {
        if (this._stopped || this._timer)
            return;
        this._timer = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, RERUN_SECONDS, () => {
            this._timer = 0;
            this._run();
            return GLib.SOURCE_REMOVE;
        });
    }

    write(message) {
        const line = new TextEncoder().encode(`${JSON.stringify(message)}\n`);
        try {
            this._proc.get_stdin_pipe().write_all(line, null);
        } catch (e) {
            logError(e, 'hunch: hunch-host went away');
        }
    }

    stop() {
        this._stopped = true;
        if (this._timer)
            GLib.source_remove(this._timer);
        this._cancel.cancel();
        this._proc?.force_exit();
    }
}

class Failure extends Error {
    constructor(message, status = 1) {
        super(message);
        this.status = status;
    }
}

function wait(ms) {
    return new Promise(resolve => GLib.timeout_add(GLib.PRIORITY_DEFAULT, ms, () => {
        resolve();
        return GLib.SOURCE_REMOVE;
    }));
}

/// GNOME's desktop as WM.md has it
class Desktop {
    constructor(pill) {
        this._pill = pill;
        const seat = Clutter.get_default_backend().get_default_seat();
        this._keyboard = seat.create_virtual_device(Clutter.InputDeviceType.KEYBOARD_DEVICE);
    }

    _app(window) {
        return Shell.WindowTracker.get_default().get_window_app(window);
    }

    _summary(window) {
        const pid = window.get_pid();
        return {
            id: String(window.get_id()),
            app: this._app(window)?.get_name() ?? window.get_wm_class() ?? '',
            title: window.get_title() ?? '',
            pid: pid > 0 ? pid : null,
            tty: null,
            host: null,
        };
    }

    _windows() {
        return global.get_window_actors()
            .map(actor => actor.meta_window)
            .filter(window => !window.is_skip_taskbar());
    }

    _window(id) {
        const window = this._windows().find(w => String(w.get_id()) === id);
        if (!window)
            throw new Failure(`no window ${id}`, GONE);
        return window;
    }

    /// The window the human sees at x, y: the topmost one there
    _windowAt(x, y) {
        const stacked = global.display.sort_windows_by_stacking(this._windows());
        return stacked.reverse().find(w => {
            const r = w.get_frame_rect();
            return w.showing_on_its_workspace() && x >= r.x && x < r.x + r.width &&
                y >= r.y && y < r.y + r.height;
        });
    }

    /// What the human calls a window: its app, and its title
    describe(id) {
        const window = this._windows().find(w => String(w.get_id()) === id);
        if (!window)
            return `window ${id}`;
        const {app, title} = this._summary(window);
        return title ? `${app} — ${title}` : app;
    }

    context() {
        const workspace = global.workspace_manager.get_active_workspace();
        const focused = global.display.get_focus_window();
        const [x, y] = global.get_pointer();
        const under = this._windowAt(x, y);
        return {
            wm: 'gnome',
            workspace: {
                id: String(workspace.index()),
                name: `Workspace ${workspace.index() + 1}`,
                display: String(global.display.get_primary_monitor()),
            },
            window: focused ? this._summary(focused) : null,
            pointer: {
                x, y,
                display: String(global.display.get_current_monitor()),
                window: under ? String(under.get_id()) : null,
            },
        };
    }

    displays() {
        const primary = global.display.get_primary_monitor();
        return [...Array(global.display.get_n_monitors()).keys()].map(i => {
            const r = global.display.get_monitor_geometry(i);
            return {id: String(i), name: null, x: r.x, y: r.y, width: r.width, height: r.height, main: i === primary};
        });
    }

    windows() {
        const active = global.workspace_manager.get_active_workspace();
        return this._windows().map(window => {
            const r = window.get_frame_rect();
            const state = window.is_fullscreen() ? 'fullscreen'
                : window.minimized ? 'minimized'
                    : window.maximized_horizontally && window.maximized_vertically ? 'maximized'
                        : 'normal';
            return {
                ...this._summary(window),
                workspace: window.get_workspace() ? String(window.get_workspace().index()) : null,
                x: r.x, y: r.y, width: r.width, height: r.height,
                state,
                visible: !window.minimized && window.located_on_workspace(active),
                focused: window.has_focus(),
            };
        });
    }

    _focus(window) {
        if (window.has_focus())
            return Promise.resolve();
        return new Promise((resolve, reject) => {
            const display = global.display;
            let timer = 0;
            const changed = display.connect('notify::focus-window', () => {
                if (!window.has_focus())
                    return;
                display.disconnect(changed);
                GLib.source_remove(timer);
                resolve();
            });
            timer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, FOCUS_MS, () => {
                display.disconnect(changed);
                reject(new Failure(`${this.describe(String(window.get_id()))} didn't take the focus`));
                return GLib.SOURCE_REMOVE;
            });
            window.activate(global.get_current_time());
        });
    }

    /// GNOME lets no program type into a window but the one in focus, so
    /// the window gets it while the keys go there, then the one that had
    /// it gets it back. Never into the lock screen, and never while the
    /// Shell itself has the keys.
    async _typing(id, type) {
        const window = this._window(id);
        if (Main.sessionMode.isLocked)
            throw new Failure('the screen is locked');
        if (Main.overview.visible || Main.modalCount > 0)
            throw new Failure('GNOME Shell has the keys: close the overview or the dialog');
        const had = global.display.get_focus_window();
        await this._focus(window);
        try {
            await type(window);
        } finally {
            if (had && had !== window && this._windows().includes(had))
                had.activate(global.get_current_time());
        }
    }

    _tap(...keyvals) {
        const time = GLib.get_monotonic_time();
        for (const keyval of keyvals)
            this._keyboard.notify_keyval(time, keyval, Clutter.KeyState.PRESSED);
        for (const keyval of keyvals.reverse())
            this._keyboard.notify_keyval(time, keyval, Clutter.KeyState.RELEASED);
    }

    /// Pastes text, since typing it key by key would take keys the
    /// keyboard's layout lacks; a terminal pastes with Shift too
    send(id, text) {
        return this._typing(id, async window => {
            const terminal = (this._app(window)?.get_app_info()?.get_categories() ?? '')
                .split(';').includes('TerminalEmulator');
            const clipboard = St.Clipboard.get_default();
            const held = await new Promise(resolve =>
                clipboard.get_text(St.ClipboardType.CLIPBOARD, (_, t) => resolve(t)));
            clipboard.set_text(St.ClipboardType.CLIPBOARD, text);
            if (terminal)
                this._tap(Clutter.KEY_Control_L, Clutter.KEY_Shift_L, Clutter.KEY_v);
            else
                this._tap(Clutter.KEY_Control_L, Clutter.KEY_v);
            await wait(RESTORE_MS);
            clipboard.set_text(St.ClipboardType.CLIPBOARD, held ?? '');
        });
    }

    submit(id) {
        return this._typing(id, async () => {
            this._tap(Clutter.KEY_Return);
            await wait(RESTORE_MS);
        });
    }

    focus(id) {
        Main.overview.hide();
        return this._focus(this._window(id));
    }

    /// Lets the human click a window; Escape cancels
    pick() {
        if (this._picking)
            return Promise.reject(new Failure('the human picks a window already'));
        Main.overview.hide();
        const catcher = new St.Widget({
            reactive: true,
            x: 0, y: 0,
            width: global.stage.width, height: global.stage.height,
        });
        Main.layoutManager.addTopChrome(catcher);
        const grab = Main.pushModal(catcher);
        this._pill.show('Click a window · Esc cancels', 'picking');
        this._picking = true;
        return new Promise((resolve, reject) => {
            const end = settle => {
                this._picking = false;
                Main.popModal(grab);
                catcher.destroy();
                this._pill.done('picking');
                settle();
            };
            catcher.connect('button-press-event', (_, event) => {
                const [x, y] = event.get_coords();
                const window = this._windowAt(x, y);
                end(() => window ? resolve(String(window.get_id()))
                    : reject(new Failure('no window there', CANCELLED)));
                return Clutter.EVENT_STOP;
            });
            catcher.connect('key-press-event', (_, event) => {
                if (event.get_key_symbol() === Clutter.KEY_Escape)
                    end(() => reject(new Failure('no window picked', CANCELLED)));
                return Clutter.EVENT_STOP;
            });
        });
    }

    async answer({command, args = [], input = null}) {
        switch (command) {
        case 'context': return this.context();
        case 'displays': return this.displays();
        case 'windows': return this.windows();
        case 'send': return await this.send(args[0], input ?? '') ?? null;
        case 'submit': return await this.submit(args[0]) ?? null;
        case 'focus': return await this.focus(args[0]) ?? null;
        case 'pick': return await this.pick();
        default: throw new Failure(`${command} isn't implemented for gnome yet`);
        }
    }

    destroy() {
        this._keyboard.run_dispose();
    }
}

/// What the seat is doing, at the bottom of the screen, as the Mac's pill
class Pill {
    constructor() {
        this._label = new St.Label({style_class: 'hunch-pill', visible: false});
        Main.layoutManager.addTopChrome(this._label);
        this._mode = null;
    }

    show(text, mode) {
        this._mode = mode;
        this._label.text = text;
        this._label.visible = true;
        const monitor = Main.layoutManager.primaryMonitor;
        const [, width] = this._label.get_preferred_width(-1);
        const [, height] = this._label.get_preferred_height(-1);
        this._label.set_position(
            Math.floor(monitor.x + (monitor.width - width) / 2),
            Math.floor(monitor.y + monitor.height - height - 48));
    }

    /// Hides it, if it still shows mode
    done(mode) {
        if (this._mode === mode) {
            this._mode = null;
            this._label.visible = false;
        }
    }

    seat(seat, error) {
        if (seat.opening)
            this.show('Opening the microphone…', 'seat');
        else if (seat.recording)
            this.show('Listening…', 'seat');
        else if (seat.transcribing)
            this.show('Transcribing…', 'seat');
        else if (seat.loading)
            this.show('Loading model…', 'seat');
        else if (error)
            this.error(error);
        else
            this.done('seat');
    }

    error(message) {
        this.show(message, 'error');
        GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, ERROR_SECONDS, () => {
            this.done('error');
            return GLib.SOURCE_REMOVE;
        });
    }

    destroy() {
        this._label.destroy();
    }
}

const Indicator = GObject.registerClass(
class Indicator extends PanelMenu.Button {
    _init(extension, desktop, pill) {
        super._init(0.0, 'hunch');
        this._extension = extension;
        this._desktop = desktop;
        this._pill = pill;
        const icon = Gio.icon_new_for_string(`${extension.path}/icons/hunch-symbolic.svg`);
        this._icon = new St.Icon({gicon: icon, style_class: 'system-status-icon'});
        this.add_child(this._icon);
        this._seat = null;
        this._data = null;
        // GNOME won't open an empty menu, so it shows what it last knew,
        // then what's true now
        this._build();
        this._refresh().catch(e => logError(e, 'hunch: the menu'));
        this.menu.connect('open-state-changed', (_, open) => {
            if (open)
                this._refresh().catch(e => logError(e, 'hunch: the menu'));
        });
    }

    show(seat) {
        // A new error shows once, as it comes
        const error = seat.error !== this._seat?.error ? seat.error : null;
        this._seat = seat;
        this._icon.style_class = seat.recording ? 'system-status-icon hunch-recording'
            : seat.opening || seat.transcribing || seat.loading ? 'system-status-icon hunch-busy'
                : 'system-status-icon';
        this._pill.seat(seat, error);
    }

    _item(menu, text, action = null) {
        const item = new PopupMenu.PopupMenuItem(text, {reactive: action !== null});
        if (action)
            item.connect('activate', () => action().catch(e => this._pill.error(e.message)));
        menu.addMenuItem(item);
        return item;
    }

    _choices(title, choices, chosen, choose) {
        const submenu = new PopupMenu.PopupSubMenuMenuItem(title);
        for (const [value, label] of choices) {
            const item = this._item(submenu.menu, label, () => choose(value));
            item.setOrnament(value === chosen ? PopupMenu.Ornament.CHECK : PopupMenu.Ornament.NONE);
        }
        this.menu.addMenuItem(submenu);
        return submenu;
    }

    async _status() {
        try {
            const needs = JSON.parse(await hunchHost(['needs']));
            return needs.length === 0 ? 'Running' : `Needs ${needs.map(n => n.grant).join(', ')}`;
        } catch (e) {
            return 'Not running';
        }
    }

    /// Asks hunch-host what the menu shows, all at once, since another
    /// host's models come over ssh
    async _refresh() {
        // None until the human picks one, on a host without a compute
        const [host, addr] = this._seat?.compute?.split(' ') ?? [null, null];
        const [status, roles, allow, models, browser] = await Promise.all([
            this._status(),
            hunchHost(['roles']).then(JSON.parse, () => []),
            micAllow(),
            host && hunchHost(['compute', addr, 'models'], {host}).then(JSON.parse, e => e),
            hunchHost(['browser', 'status']).then(JSON.parse, () => null),
        ]);
        this._data = {status, roles, allow, host, addr, models, browser};
        this._build();
    }

    _build() {
        this.menu.removeAll();
        const data = this._data;
        const seat = this._seat;
        this._item(this.menu, data?.status ?? 'Asking hunch-host…');
        if (data && seat) {
            this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem('Dictation'));
            this._item(this.menu, 'Copy Last Transcription', async () => {
                const text = await hunchHost(['seat', 'last']);
                St.Clipboard.get_default().set_text(St.ClipboardType.CLIPBOARD, text);
            });
            // Where the speech goes, among the hosts the human allowed
            const hosts = data.roles.includes('compute') ? [LOCALHOST, ...data.allow] : data.allow;
            const computes = hosts.map(h => [`${h} ${LOCAL}`, hostName(h)]);
            if (hosts.length === 0)
                this._item(this.menu, 'Allow a Host to Transcribe On…', async () => this._extension.openPreferences());
            else
                this._allowed(this._choices(data.host ? `Speech to ${hostName(data.host)}` : 'Speech to…',
                    computes, seat.compute, compute => hunchHost(['seat', 'compute', ...compute.split(' ')])));
            if (data.host)
                this._model(seat, data.host, data.addr, data.models);
            if (seat.target === 'focus') {
                this._item(this.menu, 'Redirect to Window…', async () => {
                    const window = await this._desktop.pick();
                    await hunchHost(['seat', 'target', LOCALHOST, 'gnome:', window]);
                });
            } else {
                const [, addr, window] = seat.target.split(' ');
                const name = addr === 'gnome:' ? this._desktop.describe(window) : seat.target;
                this._item(this.menu, `Stop Redirect to ${name}`,
                    () => hunchHost(['seat', 'target', 'focus']));
            }
        }
        if (data?.browser)
            this._browser(data.browser);
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        this._item(this.menu, 'Settings…', async () => this._extension.openPreferences());
    }

    /// Ends SUBMENU of hosts with where the human allows them
    _allowed(submenu) {
        submenu.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        this._item(submenu.menu, 'Allowed Hosts…', async () => this._extension.openPreferences());
    }

    _model(seat, host, addr, models) {
        if (models instanceof Error) {
            this._item(this.menu, `No models on ${hostName(host)}: ${models.message}`);
            return;
        }
        const model = models.find(m => m.id === seat.model);
        const title = model ? model.name ?? model.id : 'No model';
        // A loaded model takes about its file's size in memory
        const state = seat.loading ? ' · loading'
            : model?.loaded ? ` · ${GLib.format_size(model.size)}` : '';
        this._choices(title + state, models.map(m => [m.id, m.name ?? m.id]), seat.model,
            id => hunchHost(['seat', 'model', id]));
        if (!model || seat.loading)
            return;
        if (model.loaded)
            this._item(this.menu, 'Eject', () => hunchHost(['compute', addr, 'unload'], {host}));
        else
            this._item(this.menu, 'Load', () => hunchHost(['compute', addr, 'load', model.id], {host}));
    }

    _browser(browser) {
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem('Browser'));
        if (browser.browsers.length === 0) {
            this._item(this.menu, 'No browser connected');
            return;
        }
        // What goes where, as for dictation; only the hosts the human
        // allowed, as SEAT.md says
        const submenu = new PopupMenu.PopupSubMenuMenuItem(
            `${browser.browsers.join(', ')} · pages to ${hostName(browser.serves)}`);
        for (const host of [LOCALHOST, ...browser.allow]) {
            const item = this._item(submenu.menu, hostName(host), () => hunchHost(['browser', 'serve', host]));
            item.setOrnament(host === browser.serves ? PopupMenu.Ornament.CHECK : PopupMenu.Ornament.NONE);
        }
        this._allowed(submenu);
        this.menu.addMenuItem(submenu);
    }
});

export default class HunchExtension extends Extension {
    enable() {
        this._pill = new Pill();
        this._desktop = new Desktop(this._pill);
        this._indicator = new Indicator(this, this._desktop, this._pill);
        Main.panel.addToStatusArea(this.uuid, this._indicator);
        this._seat = new Lines(['hunch-host', 'seat', 'watch'], seat => this._indicator.show(seat));
        this._connection = new Lines(['hunch-host', 'wm', 'gnome:', 'connect'], message => {
            this._desktop.answer(message)
                .then(result => this._connection.write({id: message.id, result}))
                .catch(e => this._connection.write({
                    id: message.id, error: e.message, status: e.status ?? 1,
                }));
        });
    }

    disable() {
        this._connection.stop();
        this._seat.stop();
        this._indicator.destroy();
        this._desktop.destroy();
        this._pill.destroy();
        this._connection = this._seat = this._indicator = this._desktop = this._pill = null;
    }
}
