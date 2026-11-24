# Browser protocol

The browser is the web browser the human works in. Its extension serves
it: the browser runs hunch-browser-native, as the native messaging host
`hunch`, which passes the extension the commands below and serves the
browser's files (see Files). No port is open and no secret is kept: only
the browser runs the bridge, and only the human reaches its socket.

Terms:

    TAB         a tab, the browser's BUFFER, as TREE.md says
    SELECTOR    a CSS selector

Commands:

    context             where the human is: tab, title, selection
    selection           the selected text in the active tab
    read                the text of the active tab
    dom [SELECTOR]      HTML of the elements SELECTOR matches in the active
                        tab, or of the whole page; without scripts, styles
                        and SVG

`context` prints one JSON object, where the human is:

    tab         the active TAB
    title       its title
    selection   the selection in the page, or null: text, selector of the
                nearest block around it and that block's html, null when
                too large

```json
{
  "tab": 7, "title": "Example",
  "selection": {
    "text": "emphasis", "selector": "#b",
    "html": "<p id=\"b\">Second <em>emphasis</em> here.</p>"
  }
}
```

Output: a command prints its result as text; JSON values other than
strings are printed as JSON. A command without a result prints nothing.

Rules:

- Commands only read: none changes a page or what the human sees.
- Only the active page's content leaves the browser: no addresses, no
  other tabs.
- The extension never asks the human anything.

## Native messaging

Each message is a JSON object, its length before it in 4 bytes of native
byte order:

- the extension says `{"hello": {"name": "Firefox"}}` first
- a command comes as `{"id": 3, "command": "dom", "args": ["#b"]}`
- the extension replies `{"id": 3, "result": ...}` or `{"id": 3,
  "error": "..."}`
- `{"focus": true}` says the human switched to this browser: its bridge
  takes the socket, so commands go to the browser used last

The human lets the browser run the bridge once, with
`hunch-browser-native --install`, unless a package installed it for all
users.

## Files

`seat/browser/` is the browser used last, which hunch-browser-native
serves at ROOT/sock/browser.sock; in the current activity's workspace
only (TREE.md). Its files are TREE.md's Buffers, and dom. The only
buffer is the active tab, as the rules say: context is the command's,
with its TAB as buffer, and body is read.

    seat/browser/
      N/
        dom         read: dom; write SELECTOR first for dom SELECTOR

Errors:

    ENOENT      no browser runs the extension, the activity isn't
                current, or N isn't the active tab
    EIO         the extension failed; errstr says why
