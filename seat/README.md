# hunch-seat

Serves the seat of a host to hunch: the window managers it types into,
local:, the speech models this host runs, and dictation.

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

    hunch-seat roles    the roles this build serves, as a JSON array,
                        e.g. ["wm"] on a host that runs no models
    hunch-seat wms      the ADDR of each wm not inside another one, as a
                        JSON array
    hunch-seat needs [--ask] [--all]
                        what the human has yet to grant the service, as a
                        JSON array: for each, what it's `for` and the
                        `grant` in the OS's terms, e.g. Screen Recording;
                        --ask has the OS ask the human, where it can;
                        --all lists every grant, each with `granted`

The service never grants itself anything: the human does.

## Models

The service serves local:, as
[INFERENCE.md](../skills/hunch/INFERENCE.md) says, at
`ROOT/sock/inference.sock`, for hunch-host to mount: speech models
through transcribe.cpp, the GGUF files whose metadata has `stt.` keys.
It keeps the model it loaded between requests.

It finds them in the Hugging Face cache, `$HF_HOME/hub`, by default
`~/.cache/huggingface/hub`, so a model is never downloaded twice,
whichever tool fetched it. `~/.config/hunch/models` lists more model
directories, one per line, e.g. LM Studio's; a model there is a `.gguf`
file. A MODEL is a Hugging Face repo, e.g.
handy-computer/parakeet-tdt-0.6b-v3-gguf, with :QUANT where a repo has
several quantizations, as llama.cpp names them, e.g. Q8_0; or a path
inside a model directory.

## Dictation

The seat transcribes through the workspace's `inference/`, whichever
ENDPOINT it mounts, and types the text into the window in focus, or
into the one the human redirected it to. It sends speech as the first of
the model's inputs it writes: Ogg Opus, a tenth of WAV's size, or WAV.

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

On Linux the dictation key comes from evdev: the human joins the group
input. Each role is a Cargo feature, all on by default; a seat that
runs no models, as a thin laptop, leaves inference out:

    cargo install --path . --no-default-features --features seat

To try it by hand:

    hunch-seat serve &
    hunch-seat wm "tmux:$(tmux display -p '#{socket_path}')" windows
    hunch-9p read inference/models
    hunch-seat seat context

To test it, with tmux and jq installed:

    cargo test
    test/run
