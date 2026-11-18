# The tray

The tray is where the human sees and sets the seat: an icon in the menu
bar on a Mac, in the top bar on GNOME. It's the same on every OS, each
drawing it with its own toolkit. It holds nothing itself: the seat
([SEAT.md](../skills/hunch/SEAT.md)) holds the state, the seat's config
the allows, and the tray only calls `hunch-host`.

Terms:

    This Computer   the host the tray runs on; This Mac on a Mac
    allow           the hosts the human lets a device's data go to; a
                    host in it is one they reach by ssh HOST without a
                    password, and that runs hunch-host

## Menu

    Running
    ── Dictation
    Copy Last Transcription
    Transcribe on: HOST          ▸ This Computer, allowed hosts, Allow Hosts…
    Model: NAME · SIZE           ▸ the models on HOST, Load or Eject
    Type into: WINDOW            ▸ Focused Window, Pick a Window…
    ── Browser
    BROWSERS · used by: HOST     ▸ This Computer, allowed hosts, Allow Hosts…
    ──
    Settings…

The first line is the service's state: Running, Needs GRANT when the
human has yet to grant it something, or Not running.

Transcribe on: the host that turns speech into text. The recording goes
there over ssh. The submenu offers This Computer if it runs models, and
the hosts in the microphone's allow. If there's none, the item is Allow
a Host to Transcribe On… instead.

Model: the model on that host; SIZE, the memory it takes, only while
it's loaded. Load and Eject load it into memory or free it now, rather
than at the next dictation or after it's been idle a while.

Type into: where the text goes. Focused Window is the window in focus
when the human releases the key. Pick a Window… waits for a click on a
window, which then gets the text wherever the focus is; Esc cancels the
pick.

used by: the host whose agents may read and act on the browser's pages.
BROWSERS are the browsers connected; with none the item is No browser
connected, followed by Install for Firefox where the tray can install
it.

Allow Hosts… opens Settings on Hosts.

## Settings

    Dictation      Key, Press Enter after the text, Load the model at
                   start, Keep the model ready
    Hosts          Transcribe on: the microphone's allow
                   Browser used by: the browser's allow
    Permissions    what the service needs, as hunch-host needs --all
                   lists it
    About

An OS adds only what it alone has: a Mac a General page first, with Show
in Dock, and Quit at the end of the menu.

## Pill

While the seat works, a pill at the bottom of the screen says what it
does: Opening the microphone…, Listening…, Transcribing…, Loading
model…, Click a window · Esc cancels. When something fails, it shows the
reason, on one line, for a few seconds.
