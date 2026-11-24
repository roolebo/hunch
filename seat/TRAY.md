# The tray

The tray is where the human sees and sets the seat: an icon in the menu
bar on a Mac, in the top bar on GNOME. It holds nothing itself. The seat
writes the menu, `seat/menu` in the host's workspace as
[SEAT.md](../skills/hunch/SEAT.md) says, and the tray draws it with its
OS's toolkit; a click writes the item's line to `seat/ctl`. So the menu
is the same on every OS. The settings read and write the seat's files
too; only the pill follows `hunch-seat seat watch`, and a Mac asks for
grants with `hunch-seat needs --ask`, which only the service can.

Terms:

    This Computer, local:, where it runs models; or a
                    server, by its URL
    This Computer   the host the tray runs on; This Mac on a Mac

## Menu

What the seat writes:

    Running
    ── Dictation
    Copy Last Transcription
    Redirect to Window…
    ──
    Settings…

The first line is Running, or Needs GRANT when the human has yet to grant
the service something. A tray that can't read the menu shows Not
running and Settings….

The menu has what the human changes often, the window the text goes
to; what they set once, where speech is transcribed and with which
model, is in Settings.

Redirect to Window… waits for a click on a window, which then gets the
text wherever the focus is; Esc cancels. Then the item is Stop Redirect
to WINDOW, and the text goes to the window in focus again.

## Settings

The tray's own, since they edit what only the human sets:

    Dictation      Model: those of inference/ that take audio and answer
                   text; Key, Press Enter after the text, Load the model
                   at start, Keep the model ready
    Inference      Transcribe on: This Computer, local:, where it runs
                   models, or a server, by its URL and the key this
                   host logs in with; the tray writes the mount into the
                   namespace
    Permissions    what the service needs, as hunch-seat needs --all
                   lists it
    About

An OS adds only what it alone has: a Mac a General page first, with Show
in Dock, and Quit at the end of the menu.

## Pill

While the seat works, a pill at the bottom of the screen says what it
does, as `hunch-seat seat watch` tells it: Opening the microphone…,
Listening…, Transcribing…, Loading model…, Click a window · Esc cancels.
When something fails, it shows the reason, on one line, for a few
seconds.
