# hunch-host

Serves what a host has to hunch, as the protocols in
[skills/hunch](../skills/hunch) specify: the host's window managers,
its compute, its seat and the browser used at it.

    hunch-host serve
    hunch-host wm ADDR COMMAND [ARGUMENTS]
    hunch-host compute ADDR COMMAND [ARGUMENTS]
    hunch-host seat COMMAND [ARGUMENTS]
    hunch-host browser COMMAND [ARGUMENTS]

Every command but serve asks the service, which runs it on the caller's
stdin, stdout and stderr, so the service has to run first.

On macOS, the Hunch app in [macos](../macos) runs the service and starts
it at login. To run it at login without the app, on macOS:

    cargo install --path .
    sed "s|@HUNCH_HOST@|$(command -v hunch-host)|; s|@HOME@|$HOME|" \
        dist/hunch-host.plist > ~/Library/LaunchAgents/hunch-host.plist
    launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/hunch-host.plist

On Linux:

    cargo install --path .
    sed "s|@HUNCH_HOST@|$(command -v hunch-host)|" dist/hunch-host.service \
        > ~/.config/systemd/user/hunch-host.service
    systemctl --user enable --now hunch-host

macOS ties a grant to the binary's signature, and cargo signs each
build anew, so a rebuilt binary needs its grants again unless it's
signed with a certificate:

    codesign -f -s CERTIFICATE "$(command -v hunch-host)"

Window managers:

- tmux, on any Unix: every command but pick
- macOS: context, displays, windows, send, submit and pick
- GNOME, through the Shell extension in [gnome](../gnome): context,
  displays, windows, focus, and send and submit to the window in focus

A browser's extension reaches the service by native messaging, as the
host `hunch`: the browser starts `hunch-host-native`, a link to
hunch-host that `hunch-host browser install` makes. Each message is a
JSON object, its length before it in 4 bytes of native byte order:

- the extension says `{"hello": {"name": "Firefox"}}` first
- a command comes as `{"id": 3, "command": "dom", "args": ["#b"]}`, a
  command of [BROWSER.md](../skills/hunch/BROWSER.md)
- the extension replies `{"id": 3, "result": ...}` or `{"id": 3,
  "error": "..."}`
- `{"focus": true}` says the human switched to this browser: commands
  go to the browser used last

A seat links to each host its config allows (see
[SEAT.md](../skills/hunch/SEAT.md)) by `ssh HOST hunch-host link`. So
on HOST, the service runs and hunch-host is on the PATH ssh gives a
command; and ssh logs in without asking anything, as with a key the
agent holds.

Each role is a Cargo feature, all on by default; a build can leave out
the ones a host doesn't need, e.g. `cargo build --no-default-features
--features wm`.

To test it, with tmux and jq installed:

    cargo test
    test/run
