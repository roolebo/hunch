# Seat protocol

The seat is where the human sits: the keyboard, microphone and display
of the machine in front of them. It takes dictation: while the human
holds the dictation key it records, has the workspace's inference
transcribe the speech, and types the text into the window in focus, or
into the window the human redirected it to. hunch-seat serves it; on a
Mac the Hunch app runs it, on GNOME its Shell extension draws it.

Terms:

    MODEL       a model of the workspace's inference that takes audio and
                answers text, by its id, as INFERENCE.md says
    WM          the seat's window manager, as WM.md in hunch-seat says,
                e.g. macos:
    TARGET      where the text goes: focus, the window in focus when the
                human releases the key; or WM WINDOW, a window the human
                picked

## Files

`seat/` is the seat of the host. Its own files belong to no activity, so
an activity's workspace has them only while the activity is current;
attached without an activity, as the tray does, a tool always has them.
What's on the seat is mounted beside them, as TREE.md says: the
activity's `editor/` and the `browser/`.

    seat/
      context     read: what the seat does and how it's set, as JSON
      last        read: the last transcription
      menu        read: the tray's menu, as JSON: what the human
                  changes often; the rest is in the tray's settings
      devices     read: the microphones, as JSON: id, name and in_use of
                  each
      needs       read: each grant the seat uses, as JSON: what it's
                  for, the grant in the OS's terms, and whether it has it
      ctl         write a command, one per line
      wm/         the WM's windows, as TREE.md's Buffers: context is
                  WM.md's, with the focused WINDOW as buffer; index lists
                  the windows; body is what a window shows, as PNG;
                  nothing is selected. A window that runs a wm, as
                  WM.md's inside says, holds the windows that wm shows
                  in it

context holds:

    recording, opening, transcribing, loading, picking
                what the seat does now: records while the human holds the
                key; opens the microphone, which can take seconds after
                sleep; transcribes; loads the model; waits for the human
                to click the window to redirect to
    model       the MODEL, or null until the human picks one
    target      the TARGET
    key         the dictation key, e.g. right-super
    submit      whether Enter follows the text
    error       why the last dictation failed, or null

```json
{
  "recording": false, "opening": false, "transcribing": false,
  "loading": false, "picking": false,
  "model": "handy-computer/parakeet-ultra-gguf",
  "target": "focus", "key": "right-super", "submit": true, "error": null
}
```

ctl takes:

    target focus            type into the window in focus
    redirect                let the human click the window to type into
    model MODEL             transcribe with MODEL
    load, eject             load the model now, or free its memory
    key KEY, submit on|off, preload on|off, warm MINUTES|always
                            what the tray's settings set

menu is an array of items, each one of:

    {"title": T}                    a line that only shows
    {"separator": HEADING}          a separator, with HEADING unless
                                    it's empty
    {"title": T, "ctl": LINE}       writes LINE to ctl
    {"title": T, "copy": FILE}      copies FILE of the seat
    {"title": T, "settings": PAGE}  opens PAGE of the tray's settings, the
                                    first for an empty one

An item may also have `checked` and `items`, its submenu.

```json
[
  {"title": "Running"},
  {"separator": "Dictation"},
  {"title": "Copy Last Transcription", "copy": "last"},
  {"title": "Redirect to Window…", "ctl": "redirect"},
  {"separator": ""},
  {"title": "Settings…", "settings": ""}
]
```

Rules:

- Dictation goes where the human meant it to: through the inference the
  namespace mounts, to TARGET. If it can't, it fails and says so; it
  never falls back to another model or another window.
- Escape while the key is held cancels the recording. The seat keeps only
  the last transcription, in memory.

Errors:

    ENOENT      the activity isn't current
    EINVAL      ctl: no command
    EIO         hunch-seat failed or isn't running; errstr says why
