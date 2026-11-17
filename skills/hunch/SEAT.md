# Seat protocol

A seat is where the human sits: the devices of a host they use in person,
its keyboards, pointers, microphones, cameras, displays and speakers. The
human sits at one seat at a time. A host they only reach over ssh or a
remote desktop isn't their seat then, though it is when they sit at its
own console.

The seat takes dictation: while the human holds the dictation key it
records the microphone, then has a compute transcribe the recording and
sends the text to the target. It also tells the human things, by voice
or on a display. It shows a tray icon.

hunch-host serves the seat of its host (see HOST.md):

    hunch-host seat COMMAND [ARGUMENTS]

Terms:

    IDLE        seconds since the human last showed they're at the seat:
                used its keyboard or pointer, or moved where the seat
                can see
    DEVICE      a device, by an id the seat gives it; its kind is one of
                keyboard, pointer, mic, camera (input) or display, speaker
                (output); more kinds may come, and a client skips a kind
                it doesn't know
    COMPUTE     the compute that transcribes, as HOST ADDR (see
                COMPUTE.md)
    MODEL       a model of the seat's compute
    TARGET      where dictation goes: focus, the window in focus when the
                human releases the key; or HOST ADDR WINDOW, a window of a
                wm (see WM.md)
    KEY         the dictation key, a modifier that types nothing alone:
                right-command, right-option, right-control, right-shift
                or fn
    MINUTES     a whole number of minutes
    TEXT        text, passed on stdin

Commands:

    context             the seat's state
    watch               the state again each time it changes, a line
                        each, until the client goes
    last                the last transcription
    devices             the devices
    use DEVICE          use DEVICE for its kind, e.g. record from a mic
    compute COMPUTE     transcribe with COMPUTE
    model MODEL         transcribe with MODEL
    target TARGET       send dictation to TARGET
    key KEY             dictate while KEY is held
    submit on|off       press Enter after the text, or not
    preload on|off      load MODEL at start, or at the first dictation
    warm MINUTES|always keep MODEL in memory this long while idle, from
                        the service's next start
    say                 speak TEXT through the speaker in use
    show                show TEXT on the display in use

`context` prints one JSON object:

    idle        IDLE
    locked      true while the screen is locked
    wm          the ADDR of its wm: the graphical desktop, or the tty of
                a text console
    opening     true while the human holds the dictation key and the
                microphone opens, which can take seconds after sleep
    recording   true while the human holds the dictation key and the
                microphone records
    transcribing
                true from the key's release until the text is sent
    loading     true while MODEL loads
    compute     the COMPUTE in use
    model       the MODEL in use
    target      the TARGET
    key         the KEY
    submit      whether Enter follows the text
    preload     whether MODEL loads at start
    warm        MINUTES MODEL stays in memory while idle, or null for
                always
    error       why the last dictation failed, or null

```json
{
  "idle": 3.2, "locked": false, "wm": "macos:",
  "opening": false, "recording": false, "transcribing": false,
  "loading": false,
  "compute": "localhost local:",
  "model": "handy-computer/parakeet-tdt-0.6b-v3-gguf",
  "target": "focus", "key": "right-command", "submit": true,
  "preload": true, "warm": null, "error": null
}
```

`devices` prints a JSON array of devices, each with:

    id          the DEVICE
    kind        its kind
    name        its name, as the OS shows it
    in_use      true if the seat uses it
    signal      for an input in use, false while it sends nothing or
                nothing but a blank, such as digital silence or black
                frames from a covered camera; null otherwise
    allow       for a mic or a camera, the hosts its data may go to;
                empty for the others

```json
[
  {"id": "BuiltInMicrophoneDevice", "kind": "mic", "name": "MacBook Pro Microphone", "in_use": true, "signal": true, "allow": ["thinkstation"]},
  {"id": "0x8020000005ac8514", "kind": "camera", "name": "FaceTime HD Camera", "in_use": false, "signal": null, "allow": []},
  {"id": "BuiltInSpeakerDevice", "kind": "speaker", "name": "MacBook Pro Speakers", "in_use": false, "signal": null, "allow": []},
  {"id": "AirPods-Pro", "kind": "speaker", "name": "AirPods Pro", "in_use": true, "signal": null, "allow": []}
]
```

Rules:

- By default nothing leaves the host: the compute is local, the camera is
  off and every allow is empty.
- The seat sends a device's data only to the hosts in its allow. compute
  fails if the mic's allow doesn't list COMPUTE's host.
- A recording goes to COMPUTE's host over ssh, as `hunch-host compute`
  takes it there.
- Dictation goes where the human meant it to: through COMPUTE, to
  TARGET. If it can't, it fails and the seat says so; it never falls
  back to another compute or another window.
- While a mic or a camera records, the human sees it: by the device's
  own light where it has one, and always by the seat's own sign, the tray
  icon or the indicator of the phone's OS. The seat doesn't record from a
  device it can't show this way.
- The tray shows which inputs are in use, whether they get a signal, and
  where their data goes: the speech to COMPUTE's host, the text to
  TARGET's, the browser's pages to the host it serves. For speech and
  pages it offers only the hosts in their allow.
- Only the human sets allow, in the seat's config or from the tray; no
  command sets it.
- Escape while KEY is held cancels the recording. Another key, a
  shortcut with KEY or a key some program posts, stops it: the seat
  transcribes what it got, keeps it as the last transcription and says
  it didn't send it.
- The seat keeps only the last transcription, in memory, for the human
  to copy; never on disk.

Exit status:

- 0: done
- 1: failed, the reason is on stderr
- 4: DEVICE or MODEL is unknown

## Where the human is

`hunch focus` walks from the seat down to the window the human types into:

1. The seat: of `hunch-host seats`, the one unlocked with the least
   IDLE; seats tell their hosts through their links (see HOST.md). If its
   path (below) goes through another seat's host, as with a remote
   desktop, the human sits at the outer one.
2. Its wm's `context` gives the focused window.
3. Inside the window:
   - a viewer of another host: that host's graphical desktop (see
     `hunch-host wms`)
   - a terminal: the process in the foreground on its tty
     - a multiplexer (tmux, screen): the wm that client is attached to,
       CLIENT the tty
     - ssh HOST: the multiplexer on HOST, without CLIENT
   - anything else: the walk stops at this window
4. Inside, go on from step 2.

It prints the path, outermost first, one JSON object per line: host, wm
(its ADDR), workspace and window, as in `context`.

```json
{"host": "macbook-pro", "wm": "macos:", "workspace": {"id": "1", "name": "Desktop 1", "display": "1"}, "window": {"id": "1234", "app": "iTerm2", "title": "ssh thinkstation", "pid": 812, "tty": "/dev/ttys004", "host": null}}
{"host": "thinkstation", "wm": "tmux:/tmp/tmux-1000/default", "workspace": {"id": "@2", "name": "hunch:2", "display": null}, "window": {"id": "%3", "app": "vim", "title": "main.c", "pid": 4121, "tty": "/dev/pts/4", "host": null}}
```

With TARGET focus, the seat sends dictation to the last window of the path
through its wm: a pane over ssh gets it from the multiplexer on its own
host, not as keys typed into the terminal.
