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

Commands of the host itself:

    hunch-host wms      the ADDR of each wm not inside another one:
                        graphical desktops and text consoles, as a JSON
                        array
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

Its pages go to one host at a time, localhost until the human picks
another:

    hunch-host browser serve HOST   serve the agents of HOST
    hunch-host browser status       the browsers connected and the HOST
                                    served, as JSON

To serve another host, the seat's service keeps `ssh HOST hunch-host
browser relay` running, and HOST's service passes its agents' commands
through it: the connection goes out from the seat, as the human's ssh
does. serve fails for a HOST not in the browser's allow, which only the
human sets, as for a mic (see SEAT.md).

status JSON:

- `browsers`: the name of each browser connected
- `serves`: the HOST its pages go to
- `allow`: the hosts its pages may go to

```json
{"browsers": ["Firefox", "Safari"], "serves": "thinkstation", "allow": ["thinkstation"]}
```
