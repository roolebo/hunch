# Window manager protocol

A window manager (wm) shows the human's windows and knows where their
keys go: a graphical desktop (macOS, Windows, X11, a Wayland compositor),
a terminal multiplexer (tmux, GNU screen) or a bare tty. A host without a
graphical desktop still has a tty. A window can hold another wm: a
terminal running tmux, on this host or over ssh on another one, or a
viewer of a VM or a remote desktop.

A wm isn't registered. hunch-host on its host serves it (see HOST.md):

    hunch-host wm ADDR COMMAND [ARGUMENTS]

Terms:

    ADDR        a wm, as SCHEME:LOCATION; SCHEME says what kind it is,
                e.g. macos:, x11::0,
                sway:/run/user/1000/sway-ipc.1000.1234.sock,
                tmux:/tmp/tmux-1000/default,
                screen:/run/screen/S-roolebo/1234.pts-0.thinkstation,
                tty:/dev/pts/4
    DISPLAY     a monitor, by an id the wm gives it; a multiplexer or a
                tty has none
    WORKSPACE   windows shown together on a DISPLAY: a macOS Space, a
                Windows virtual desktop, an X11 or Wayland workspace, a
                tmux window, a screen session; a tty has one
    WINDOW      what takes the human's keys, by an id the wm gives it,
                unique while it exists: a graphical window, a tmux pane, a
                screen window, the tty itself; a tmux pane's id is also the
                id of the participant in it, e.g. %3
    CLIENT      a terminal attached to a multiplexer, by its tty, e.g.
                /dev/pts/1; other wms have one client and ignore CLIENT
    DIRECTION   left, right, up or down
    STATE       how a window is shown: normal; maximized, filling its
                display beside the menus and bars; fullscreen, the whole
                display alone; minimized, hidden until restored; a
                multiplexer has normal and maximized (tmux zoom)
    TEXT        text, passed on stdin

Positions and sizes are in logical pixels from the top left of the main
DISPLAY; in a multiplexer, in character cells from the top left of the
WORKSPACE.

Commands:

    context [--client CLIENT]
                        where the human is: the focused workspace and
                        window, and the pointer
    displays            all displays
    windows             all windows
    send WINDOW         type TEXT into WINDOW
    submit WINDOW       press Enter in WINDOW
    focus WINDOW        give WINDOW the focus and bring it to the front
    swap WINDOW WINDOW  exchange the places of two windows
    move WINDOW DIRECTION
                        move WINDOW to the next place in DIRECTION
    place WINDOW X Y WIDTH HEIGHT
                        put WINDOW there, or as near as the wm can
    state WINDOW STATE  show WINDOW in STATE
    pick                let the human pick a window, print its id

Without CLIENT, a multiplexer takes the client the human typed into
last.

`context` prints one JSON object:

    wm          the SCHEME
    workspace   the focused WORKSPACE: id, name and display (its DISPLAY,
                or null)
    window      the focused WINDOW: id, app, title, pid, tty and host
    pointer     where the mouse pointer is: x, y, display, and window, the
                WINDOW under it; null in a multiplexer or a tty

app is the application's name; in a multiplexer or a tty, the command in
the window's foreground. tty is the terminal's tty if the window is a
terminal and the wm knows it, otherwise null. host is the HOST the window
shows if it's a viewer of a VM or a remote desktop, otherwise null.

```json
{
  "wm": "macos",
  "workspace": {"id": "1", "name": "Desktop 1", "display": "1"},
  "window": {"id": "1234", "app": "iTerm2", "title": "vim", "pid": 812, "tty": "/dev/ttys004", "host": null},
  "pointer": {"x": 2010, "y": 402, "display": "2", "window": "1290"}
}
```

`displays` prints a JSON array of displays, each with id, name, x, y,
width, height, and main (true for the main one).

`windows` prints a JSON array of windows, each as in `context` plus:

    workspace   its WORKSPACE
    x, y, width, height
                where it is
    state       its STATE
    visible     true if the human can see it now: not behind a tab,
                minimized or on a workspace not shown
    focused     true for the focused one

Rules:

- send and submit never move the focus or bring WINDOW to the front: the
  human keeps typing where they are.
- Only focus, swap, move, place and state change what the human sees,
  and only when the human asks for it.
- A tiling wm places a window as near as its layout allows; windows shows
  where it went.
- Only pick asks the human anything.
- A tty has no send or submit: only root can type into a tty that isn't
  its own. With one window, it has no swap, move, place or state either.

Exit status:

- 0: done
- 1: failed, the reason is on stderr
- 4: a WINDOW is gone
- 5: the human cancelled pick
