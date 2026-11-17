# Browser protocol

A browser is a participant with role `browser`: the web browser the human
works in. It registers:

- `role`: `browser`
- `addr`: where to reach it, e.g. `http://127.0.0.1:8765/SECRET`
- `handler`: a program that implements the browser protocol for it

To use the browser, run its handler:

    HANDLER ADDR COMMAND [ARGUMENTS]

The browser may run on another host than the handler, e.g. on the machine
the human sits at, while the agent runs on a remote one.

Where hunch-host runs, a browser serves through it instead (see
HOST.md): when no browser registers in the agent's window, the agent
asks `hunch-host browser`, which answers for the browser that serves its
host.

Terms:

    TAB         a tab, by an id the browser gives it, unique while it's open
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
- The handler never asks the human anything.

Exit status:

- 0: done
- 1: failed, the reason is on stderr
