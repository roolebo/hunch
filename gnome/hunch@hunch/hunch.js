// What the extension and its preferences share: the files of the host's
// workspace. Only Gio and GLib, since the preferences run apart from the
// Shell.

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

/// Reads PATH of the host's workspace
export function readTree(path) {
  return run(['hunch-9p', 'read', path]);
}

/// Writes TEXT to PATH of the host's workspace, an action
export function writeTree(path, text) {
  return run(['hunch-9p', 'write', path], text);
}

/// Reads FILE of the seat
export function readSeat(file) {
  return readTree(`seat/${file}`);
}

/// Writes TEXT to FILE of the seat, an action
export function writeSeat(file, text) {
  return writeTree(`seat/${file}`, text);
}

/// Writes LINE to the seat's ctl: one of the seat's commands
export function seatCtl(line) {
  return writeSeat('ctl', line);
}

/// Runs ARGV with INPUT on its stdin; its stdout, or an Error with the
/// first line it said on stderr, from where hunch-seat says why: the
/// rest is usage, too long for a menu
function run(argv, input = null) {
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
          reject(new Error(
            err.trim().split('\n')[0].replace(/^.*hunch-seat: /, '')));
      } catch (e) {
        reject(e);
      }
    });
  });
}

/// Where the extension leaves the preferences' page to open, since GNOME
/// passes them nothing
function pagePath() {
  return GLib.build_filenamev(
    [GLib.get_user_runtime_dir(), 'hunch', 'prefs-page']);
}

export function askPage(name) {
  GLib.mkdir_with_parents(GLib.path_get_dirname(pagePath()), 0o700);
  GLib.file_set_contents(pagePath(), name);
}

/// The page the extension asked for, once; null if none
export function takePage() {
  const path = pagePath();
  if (!GLib.file_test(path, GLib.FileTest.EXISTS))
    return null;
  const [, bytes] = GLib.file_get_contents(path);
  GLib.unlink(path);
  return new TextDecoder().decode(bytes);
}
