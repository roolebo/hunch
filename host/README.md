# hunch-host

Serves what a host has to hunch, as the protocols in
[skills/hunch](../skills/hunch) specify: the host's window managers,
its compute and its seat.

    hunch-host serve
    hunch-host wm ADDR COMMAND [ARGUMENTS]
    hunch-host compute ADDR COMMAND [ARGUMENTS]
    hunch-host seat COMMAND [ARGUMENTS]

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

Each role is a Cargo feature, all on by default; a build can leave out
the ones a host doesn't need, e.g. `cargo build --no-default-features
--features wm`.

To test it, with tmux and jq installed:

    cargo test
    test/run
