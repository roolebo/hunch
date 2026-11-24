# hunch-seat

Serves the seat of a host to hunch: the window managers it types into,
the compute that runs speech models, and dictation, each a role.

    hunch-seat serve
    hunch-seat ROLE [ADDR] COMMAND [ARGUMENTS]

[WM.md](WM.md) specifies the role `wm`.

## Service

hunch-seat runs as the human's service, started at login: by the Hunch
app on a Mac, by launchd or a systemd user unit elsewhere. The service
holds what has to stay open, such as the dictation key, a recording
microphone and a loaded model, and the grants the OS gives it. Every
other command is a client: it asks the service over
`$XDG_RUNTIME_DIR/hunch/sock/seat.sock`, under `$XDG_CACHE_HOME` or
`~/.cache` where XDG_RUNTIME_DIR isn't set, and fails if no service
runs.

Commands of the service itself:

    hunch-seat roles    the roles this build serves, as a JSON array
    hunch-seat wms      the ADDR of each wm not inside another one, as a
                        JSON array
    hunch-seat needs [--ask] [--all]
                        what the human has yet to grant the service, as a
                        JSON array: for each, what it's `for` and the
                        `grant` in the OS's terms, e.g. Screen Recording;
                        --ask has the OS ask the human, where it can;
                        --all lists every grant, each with `granted`

The service never grants itself anything: the human does.

## GNOME

GNOME Shell lets only itself see and type into the windows of its
desktop, so hunch's Shell extension in [gnome](../gnome) serves that
desktop as the wm `gnome:`. It serves the desktop to the service by
running:

    hunch-seat wm gnome: serve

and answers on that command's stdin and stdout, a JSON message per line:

- the service sends a command of WM.md, its TEXT in `input`:
  `{"id": 3, "command": "send", "args": ["1234"], "input": "hello"}`
- the extension answers with the command's id: `{"id": 3, "result":
  ...}`, or `{"id": 3, "error": "...", "status": 4}` with an exit
  status of WM.md

## Install

    cargo install --path .
    sed "s|@HUNCH_SEAT@|$(command -v hunch-seat)|" dist/hunch-seat.service \
        > ~/.config/systemd/user/hunch-seat.service
    systemctl --user enable --now hunch-seat

To try it by hand:

    hunch-seat serve &
    hunch-seat wm "tmux:$(tmux display -p '#{socket_path}')" windows

To test it, with tmux and jq installed:

    cargo test
    test/run
