# Tree protocol

An agent reaches the human's editor and browser through a workspace: a
tree of files, one per activity, served over 9P2000.L, Plan 9's file
protocol. Reading `seat/editor/context` tells where the human's cursor
is; writing `open` to `seat/editor/ctl` shows them a file.

The workspace is the whole contract. Behind `seat/editor/` may be Vim or
another editor, on this host or another: the agent can't tell and
doesn't need to.

Terms:

    ACTIVITY    a part of the human's work, as a KDE Activity is: its
                editor, its browser windows, its agent; the human moves
                between activities by moving between windows. It is a
                process, as in CSP: it takes part with other systems
                through the files of its workspace, its name space, and
                the human takes part in it through the seat. It has an
                id and may have a name
    SEAT        the terminal the human works at, as in Plan 9: whatever
                device is in their hands, with what it has, a screen and
                keyboard or only a touchscreen, a microphone and speakers
                or not. One seat is attached at a time, to the activity
                the human is in
    BUFFER      what a program holds open for the human, and the context
                an agent needs from it: an editor's buffer, a browser's
                tab, a window of the seat. A program has one or several,
                each by an id it gives it, unique while it's open. A
                buffer may run a program of the same kind, which holds
                buffers in turn: 8½ runs in a window of 8½, tmux in a
                pane of tmux
    owner       of an activity's workspace, the one hunch-host that
                assembles it and gives the activity its id; the workspace's
                devices may come from several hunch-hosts
    current     the activity the human is in now

A workspace has one device of each kind:

    errstr      why the last action failed
    seat/       where the human sits, and what's on it:
      editor/   the activity's editor, as PROTOCOL.md says
      browser/  the browser, as BROWSER.md says; in the current
                activity only
    inference/  the models the namespace mounts, as INFERENCE.md says

Buffers:

A device that holds BUFFERs has these files, beside its own:

    context     read: where the human is, as JSON: buffer, the current
                BUFFER, or null; selection, what the human selected in
                it, or null; and the device's own fields
    selection   read: the selected text
    index       read: the open buffers, one per line: N, a tab, BUFFER
    N/          a buffer, by a number the owner gives it when it lists
                the buffer, kept while the owner runs
      name      read: its BUFFER
      body      read: what it holds, as the device's protocol says

A buffer that runs a program of its kind has these files for that
program under its N/, and so on down: a terminal's window holds the
panes of the tmux in it, a pane the panes of a tmux in that. A program
of another kind stays in its own device: the editor in a pane is
seat/editor/, not under the pane.

So where the human is reads the same in the editor, the browser and the
windows, at any depth.

Assembly:

- A device knows only its own files. The owner assembles the workspace,
  and only it knows all the devices in it.
- A workspace is a device too: its files are its devices' files, each
  under the device's name.
- What a device's behaviour depends on is assembled with it. The browser
  belongs to no activity: the owner composes it with the human's focus, so
  it's in the current activity only, as rio gives the keyboard to the window
  in focus. The agent the human talks to sees the page; the others
  don't.
- Two devices that answer every read and write the same way are
  interchangeable. The same tests, run through the files, check any of
  them, and check that assembling keeps the devices' behaviour.

Moving:

- When the human moves, the activity stays. They reach its terminal from
  another machine, and the devices where they now sit, such as the
  microphone or the browser, come into the workspace from that machine's
  hunch-host.
- When the activity moves, its owner changes. The new owner takes over the
  activity's id and name and assembles the workspace from the devices on its
  host. Each part moves by its own tool: git moves the working copy, the
  agent resumes its session.
- So the owner keeps what the activity is made of, not the devices' state,
  and the activity's id is the owner's: the owner maps it onto where the
  activity runs on each host.

Serve:

A device that runs apart from hunch-host serves itself to its
activity's workspace, as a Plan 9 server posts itself in /srv, and the
owner mounts it where the workspace has it: an editor on seat/editor.
Firefox's bridge serves 9P itself, at a socket of its own; a program
that doesn't, such as Vim, runs this for as long as it serves:

    hunch-9p [--activity ACTIVITY] serve KIND

The owner sends it the device's commands on that command's stdin, and
it answers on its stdout, a JSON object per line:

- `{"id": 3, "command": "open", "args": ["/src/a.c", "12"]}`, with the
  text the command takes in `input`
- `{"id": 3, "result": "..."}`, or `{"id": 3, "error": "...",
  "status": 3}` with the exit status the device's protocol gives

The owner serves the device's files from these commands, so a device
implements only its commands. When the command ends, the device is gone
from the workspace; serving the same KIND again replaces it.

Reads and writes:

- A read shows the device as it is when the file is opened.
- A write is an action. It runs when the file is closed; a file that
  answers, such as `editor/new`, runs it when read back.
- A failed action returns an errno; errstr has the words.
- The human uses the device at the same time, so a read may show changes
  the agent didn't make.

Access:

The owner listens on $XDG_RUNTIME_DIR/hunch/sock/hunch.sock; where
XDG_RUNTIME_DIR isn't set, as on macOS, under $XDG_CACHE_HOME or
~/.cache instead. A tool attaches with the activity's id as aname. Attached
without one, it gets the host's workspace: the devices that belong to no
activity, the browser and inference. From the shell, `hunch-9p` does it,
and starts hunch-host if it isn't running; in tmux, its window is the
activity:

    hunch-9p read seat/editor/context

Unlike Plan 9:

- Vim isn't rewritten to serve files: it serves its commands, and the
  owner serves the files.
- Tools speak 9P themselves: nothing has to be mounted, and agents don't
  bind.
- A file holds JSON where its content has fields.

Control:

Every action passes through the owner, and the owner decides whether it
runs. For now every action runs: only the human reaches the socket.
Later an action may wait for the human to touch a security key. Tauth is
refused until then.
