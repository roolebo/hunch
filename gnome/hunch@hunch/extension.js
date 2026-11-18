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

// How long a line of hunch-host that ended waits before it runs again,
// as when the service restarts
const RERUN_SECONDS = 3;
// How long the window has to take the text from the clipboard before the
// clipboard gets back what it held
const RESTORE_MS = 500;
// WM.md's exit statuses
const GONE = 4;

const LOCALHOST = 'localhost';
const LOCAL = 'local:';

/// A HOST as the human calls it
function hostName(host) {
    return host === LOCALHOST ? 'This Computer' : host;
}

/// Runs hunch-host ARGS, or ssh to HOST then hunch-host, with input on
/// its stdin; its stdout, or an Error with what it said on stderr
function hunchHost(args, {input = null, host = LOCALHOST} = {}) {
    const argv = host === LOCALHOST
        ? ['hunch-host', ...args]
        : ['ssh', '-o', 'BatchMode=yes', host, 'hunch-host', ...args];
    return new Promise((resolve, reject) => {
        let proc;
        try {
            proc = Gio.Subprocess.new(argv,
                Gio.SubprocessFlags.STDIN_PIPE | Gio.SubprocessFlags.STDOUT_PIPE |
                Gio.SubprocessFlags.STDERR_PIPE);
        } catch (e) {
            reject(e);
            return;
        }
        proc.communicate_utf8_async(input, null, (p, result) => {
            try {
                const [, out, err] = p.communicate_utf8_finish(result);
                if (p.get_successful())
                    resolve(out.trim());
                else
                    reject(new Error(err.trim().replace(/^hunch-host: /, '')));
            } catch (e) {
                reject(e);
            }
        });
    });
}

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

/// GNOME's desktop as WM.md has it
class Desktop {
    constructor() {
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

    context() {
        const workspace = global.workspace_manager.get_active_workspace();
        const focused = global.display.get_focus_window();
        const [x, y] = global.get_pointer();
        const under = this._windows().find(w => {
            const r = w.get_frame_rect();
            return w.showing_on_its_workspace() && x >= r.x && x < r.x + r.width &&
                y >= r.y && y < r.y + r.height;
        });
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

    /// GNOME lets no program type into another window, so only into the
    /// one in focus, and never into the lock screen
    _typable(id) {
        const window = this._window(id);
        if (Main.sessionMode.isLocked)
            throw new Failure('the screen is locked');
        // The overview's search or a dialog would get the keys instead
        if (Main.overview.visible || Main.modalCount > 0)
            throw new Failure('GNOME Shell has the keys: close the overview or the dialog');
        if (!window.has_focus())
            throw new Failure(`GNOME types only into the window in focus, not ${id}`);
        return window;
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
        const window = this._typable(id);
        const terminal = (this._app(window)?.get_app_info()?.get_categories() ?? '')
            .split(';').includes('TerminalEmulator');
        const clipboard = St.Clipboard.get_default();
        return new Promise(resolve => {
            clipboard.get_text(St.ClipboardType.CLIPBOARD, (_, held) => {
                clipboard.set_text(St.ClipboardType.CLIPBOARD, text);
                if (terminal)
                    this._tap(Clutter.KEY_Control_L, Clutter.KEY_Shift_L, Clutter.KEY_v);
                else
                    this._tap(Clutter.KEY_Control_L, Clutter.KEY_v);
                GLib.timeout_add(GLib.PRIORITY_DEFAULT, RESTORE_MS, () => {
                    clipboard.set_text(St.ClipboardType.CLIPBOARD, held ?? '');
                    resolve(null);
                    return GLib.SOURCE_REMOVE;
                });
            });
        });
    }

    submit(id) {
        this._typable(id);
        this._tap(Clutter.KEY_Return);
        return null;
    }

    focus(id) {
        const window = this._window(id);
        Main.overview.hide();
        window.activate(global.get_current_time());
        return null;
    }

    async answer({command, args = [], input = null}) {
        switch (command) {
        case 'context': return this.context();
        case 'displays': return this.displays();
        case 'windows': return this.windows();
        case 'send': return await this.send(args[0], input ?? '');
        case 'submit': return this.submit(args[0]);
        case 'focus': return this.focus(args[0]);
        default: throw new Failure(`${command} isn't implemented for gnome yet`);
        }
    }

    destroy() {
        this._keyboard.run_dispose();
    }
}

/// The seat's config, which hunch-host reads and the human writes
function configPath() {
    return GLib.build_filenamev([GLib.get_user_config_dir(), 'hunch', 'seat.json']);
}

/// The hosts some microphone may send speech to
function micHosts() {
    try {
        const [, bytes] = GLib.file_get_contents(configPath());
        const config = JSON.parse(new TextDecoder().decode(bytes));
        const hosts = Object.values(config.devices ?? {}).flatMap(d => d.allow ?? []);
        return [...new Set(hosts)];
    } catch (e) {
        return [];
    }
}

const Indicator = GObject.registerClass(
class Indicator extends PanelMenu.Button {
    _init(extension) {
        super._init(0.0, 'hunch');
        const icon = Gio.icon_new_for_string(`${extension.path}/icons/hunch-symbolic.svg`);
        this._icon = new St.Icon({gicon: icon, style_class: 'system-status-icon'});
        this.add_child(this._icon);
        this._seat = null;
        // Built each time it opens, so it shows what's true now
        this.menu.connect('open-state-changed', (_, open) => {
            if (open)
                this._fill().catch(e => logError(e, 'hunch: the menu'));
        });
    }

    show(seat) {
        this._seat = seat;
        this._icon.style_class = seat.recording ? 'system-status-icon hunch-recording'
            : seat.opening || seat.transcribing || seat.loading ? 'system-status-icon hunch-busy'
                : 'system-status-icon';
    }

    _item(menu, text, action = null) {
        const item = new PopupMenu.PopupMenuItem(text, {reactive: action !== null});
        if (action)
            item.connect('activate', () => action().catch(e => Main.notify('hunch', e.message)));
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
    }

    _status(seat) {
        if (!seat)
            return 'Not running';
        if (seat.error)
            return seat.error;
        if (seat.opening)
            return 'Opening the microphone…';
        if (seat.recording)
            return 'Listening…';
        if (seat.transcribing)
            return 'Transcribing…';
        return seat.loading ? 'Loading the model…' : 'Ready';
    }

    async _fill() {
        this.menu.removeAll();
        const seat = this._seat;
        this._item(this.menu, this._status(seat));
        if (seat) {
            this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem('Dictation'));
            this._item(this.menu, 'Copy Last Transcription', async () => {
                const text = await hunchHost(['seat', 'last']);
                St.Clipboard.get_default().set_text(St.ClipboardType.CLIPBOARD, text);
            });
            // Where the speech goes, among the hosts the human allowed
            const [host] = seat.compute.split(' ');
            const computes = [[`${LOCALHOST} ${LOCAL}`, hostName(LOCALHOST)],
                ...micHosts().map(h => [`${h} ${LOCAL}`, h])];
            this._choices(`Speech to ${hostName(host)}`, computes, seat.compute,
                compute => hunchHost(['seat', 'compute', ...compute.split(' ')]));
            await this._models(seat, host);
            if (seat.target !== 'focus') {
                this._item(this.menu, `Stop Redirect to ${seat.target}`,
                    () => hunchHost(['seat', 'target', 'focus']));
            }
        }
        await this._browser();
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        this._item(this.menu, 'Edit Allowed Hosts…', async () => {
            Gio.AppInfo.launch_default_for_uri(GLib.filename_to_uri(configPath(), null), null);
        });
    }

    async _models(seat, host) {
        let models = [];
        try {
            models = JSON.parse(await hunchHost(['compute', LOCAL, 'models'], {host}));
        } catch (e) {
            this._item(this.menu, `No models: ${e.message}`);
            return;
        }
        const name = id => models.find(m => m.id === id)?.name ?? id ?? 'No model';
        this._choices(name(seat.model), models.map(m => [m.id, m.name ?? m.id]), seat.model,
            model => hunchHost(['seat', 'model', model]));
    }

    async _browser() {
        let browser;
        try {
            browser = JSON.parse(await hunchHost(['browser', 'status']));
        } catch (e) {
            return;
        }
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem('Browser'));
        if (browser.browsers.length === 0) {
            this._item(this.menu, 'No browser connected');
            return;
        }
        // Only the hosts the human allowed, as SEAT.md says
        const hosts = [LOCALHOST, ...browser.allow].map(h => [h, hostName(h)]);
        this._choices(`${browser.browsers.join(', ')} · pages to ${hostName(browser.serves)}`,
            hosts, browser.serves, h => hunchHost(['browser', 'serve', h]));
    }
});

export default class HunchExtension extends Extension {
    enable() {
        this._desktop = new Desktop();
        this._indicator = new Indicator(this);
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
        this._connection = this._seat = this._indicator = this._desktop = null;
    }
}
