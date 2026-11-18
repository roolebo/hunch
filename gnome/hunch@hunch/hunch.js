// What the extension and its preferences share: hunch-host, and the
// seat's config. Only Gio and GLib, since the preferences run apart from
// the Shell.

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

export const LOCALHOST = 'localhost';
export const LOCAL = 'local:';

/// A HOST as the human calls it
export function hostName(host) {
    return host === LOCALHOST ? 'This Computer' : host;
}

/// Runs hunch-host ARGS, or ssh to HOST then hunch-host, with input on
/// its stdin; its stdout, or an Error with what it said on stderr
export function hunchHost(args, {input = null, host = LOCALHOST} = {}) {
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

/// The microphone in use, as the seat's devices lists it
export async function mic() {
    const devices = JSON.parse(await hunchHost(['seat', 'devices']));
    return devices.find(d => d.kind === 'mic' && d.in_use) ?? null;
}

/// The hosts the speech of the microphone in use may go to
export async function micAllow() {
    try {
        return (await mic())?.allow ?? [];
    } catch (e) {
        return [];
    }
}

/// The seat's config, which only the human writes, here through the
/// preferences; SEAT.md specifies it
export function configPath() {
    return GLib.build_filenamev([GLib.get_user_config_dir(), 'hunch', 'seat.json']);
}

/// The config, empty if there's none; it throws rather than let a write
/// replace a file it can't read
export function readConfig() {
    const path = configPath();
    if (!GLib.file_test(path, GLib.FileTest.EXISTS))
        return {};
    const [, bytes] = GLib.file_get_contents(path);
    const config = JSON.parse(new TextDecoder().decode(bytes));
    if (typeof config !== 'object' || config === null || Array.isArray(config))
        throw new Error(`${path} isn't a JSON object`);
    return config;
}

/// Changes the config with change, which gets it and edits it in place
export function editConfig(change) {
    const config = readConfig();
    change(config);
    const path = configPath();
    GLib.mkdir_with_parents(GLib.path_get_dirname(path), 0o700);
    GLib.file_set_contents(path, `${JSON.stringify(config, null, 2)}\n`);
}
