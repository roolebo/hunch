# hunch-host

hunch-host runs on each of the human's hosts and serves what the host
has: its window managers, a seat if the human can sit at it, a compute if
it's strong enough to run models. The hosts are linked: a laptop opens a
terminal to a workstation over ssh, the workstation runs a VM.

Run it on its host:

    hunch-host ROLE [ADDR] COMMAND [ARGUMENTS]

From another host, through ssh:

    ssh HOST hunch-host ROLE [ADDR] COMMAND [ARGUMENTS]

ROLE is `wm`, `compute`, `seat` or `browser`, each with its own protocol
in this directory. A wm and a compute take an ADDR, since a host can have
several; a seat and a browser don't, since a host has at most one.

## Service

hunch-host runs as the human's service on each host, started at login:
by the host's app, launchd or a systemd user unit. The service holds
what has to stay open, such as a recording microphone, and the
permissions the OS grants it. Every other command is a client: it asks
the service over its socket, and fails if no service runs.

    hunch-host serve    runs the service in the foreground

The socket is `$XDG_RUNTIME_DIR/hunch/host.sock`, or
`$TMPDIR/hunch/host.sock` where XDG_RUNTIME_DIR isn't set, as on macOS.

A tray and the tmux status line are clients too: they show the human
what the service does and what it needs.

Terms:

    HOST        a host, by the name ssh reaches it at, e.g. thinkstation;
                localhost for the host that asks

## Links

A seat's service keeps a link to each host its data may go to, the hosts
in its allows (see SEAT.md):

    ssh HOST hunch-host link

The link goes out from the seat, as the human's ssh does, so no host has
to reach a seat; ssh decides who may link, as for any login. It carries
JSON messages, one per line.

The seat says where the human is each second:

    {"seat": {"host": "macbook-pro", "locked": false, "idle": 3.2}, "browsers": ["Firefox"]}

- `seat`: the seat, as `seats` lists it
- `browsers`: the name of each browser of the seat while it serves HOST,
  else none

HOST passes a command of BROWSER.md to those browsers; the seat answers
with its id, the result or the error:

    {"id": 7, "command": "read", "args": []}
    {"id": 7, "result": "The text of the page"}
    {"id": 8, "error": "No element matches"}

Commands of the host itself:

    hunch-host wms      the ADDR of each wm not inside another one:
                        graphical desktops and text consoles, as a JSON
                        array
    hunch-host seats    the seats linked to this host and its own, as a
                        JSON array of objects: host, locked, idle (see
                        SEAT.md, null where the seat can't tell)
    hunch-host needs [--ask] [--all]
                        what the human has yet to grant the service, as a
                        JSON array; --ask has the OS ask the human for it,
                        where the OS can, so the grant lands on the service;
                        --all lists every grant the service uses

needs JSON, for each grant:

- `for`: what the grant enables
- `grant`: what to grant, in the terms of the host's OS, e.g. Screen
  Recording on macOS, group input on Linux
- `granted`: with --all, whether the human has granted it

On macOS:

```json
[
  {"for": "window titles", "grant": "Screen Recording"},
  {"for": "dictation", "grant": "Microphone"}
]
```

The service finds a need when the OS refuses it something. It never
grants itself anything: the human or the host's admin does.

## Browser

The browser the human uses at the seat serves hunch through the
service: its extension talks to hunch-host over the browser's native
messaging, so no port is open and no secret is kept. Agents ask it with
the commands of BROWSER.md:

    hunch-host browser COMMAND [ARGUMENTS]

The human lets a browser's extension reach the service once:

    hunch-host browser install BROWSER

Its pages go to one host at a time, localhost until the human picks
another:

    hunch-host browser serve HOST   serve the agents of HOST
    hunch-host browser status       the browsers connected and the HOST
                                    served, as JSON

HOST's service passes its agents' commands to the browser through the
seat's link (see Links). A command goes to the seat the human is at, the
one with the least IDLE of those that serve HOST, and there to the
browser the human used last. serve fails for a HOST not in the browser's
allow, which only the human sets, as for a mic (see SEAT.md).

status JSON:

- `browsers`: the name of each browser connected
- `serves`: the HOST its pages go to
- `allow`: the hosts its pages may go to

```json
{"browsers": ["Firefox", "Safari"], "serves": "thinkstation", "allow": ["thinkstation"]}
```

## GNOME

GNOME Shell lets only itself see and type into the windows of its
desktop, so hunch's Shell extension serves that desktop as the wm
`gnome:`. It connects to the service by running:

    hunch-host wm gnome: connect

and answers on that command's stdin and stdout, a JSON message per
line, as a link does (see Links):

- the service sends a command of WM.md, its TEXT in `input`:
  `{"id": 3, "command": "send", "args": ["1234"], "input": "hello"}`
- the extension answers with the command's id: `{"id": 3, "result":
  ...}`, or `{"id": 3, "error": "...", "status": 4}` with an exit
  status of WM.md

The extension is also the seat's tray, in the top bar: it shows what
`hunch-host seat watch` prints and runs the commands the human picks.

