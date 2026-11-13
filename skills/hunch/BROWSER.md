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

Terms:

    TAB         a tab, by an id the browser gives it, unique while it's open
    URL         a web address
    SELECTOR    a CSS selector

Commands:

    context             where the human is: tab, selection, open tabs
    selection           the selected text in the active tab
    read                the text of the active tab
    dom [SELECTOR]      HTML of the elements SELECTOR matches in the active
                        tab, or of the whole page; without scripts, styles
                        and SVG

`context` prints one JSON object, where the human is:

    tab         the active TAB
    url, title  its URL and title
    selection   the selection in the page, or null: text, selector of the
                nearest block around it and that block's html, null when
                too large
    tabs        open tabs, each with tab, url, title and active (shown in
                its window)

```json
{
  "tab": 7, "url": "https://example.com/", "title": "Example",
  "selection": {
    "text": "emphasis", "selector": "#b",
    "html": "<p id=\"b\">Second <em>emphasis</em> here.</p>"
  },
  "tabs": [
    {"tab": 7, "url": "https://example.com/", "title": "Example", "active": true}
  ]
}
```

Output: a command prints its result as text; JSON values other than
strings are printed as JSON. A command without a result prints nothing.

Rules:

- Commands only read: none changes a page or what the human sees.
- The handler never asks the human anything.

Exit status:

- 0: done
- 1: failed, the reason is on stderr
