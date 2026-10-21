# Editor protocol

An editor is a participant with role `editor`: the editor program the
human works in. It registers:

- `role`: `editor`
- `addr`: where to reach it, e.g. `vim-server:/run/user/1000/vim/VIM`
- `handler`: a program that implements the editor protocol for it

To use the editor, run its handler:

    HANDLER ADDR COMMAND [ARGUMENTS]

Only the handler depends on the editor, so the same commands work with any
editor.

Terms:

    FILE        a file, by its absolute path on the editor's host
    BUFFER      something the editor has open, by an id the editor gives
                it, unique while it's open; a file's buffer has its FILE as
                the id
    LINE, COL   a position, from 1; COL counts characters, not bytes or
                screen columns
    OLD, NEW, TEXT
                text, passed in a file, never on the command line

Commands:

    context                 where the human is: file, cursor, selection, open files
    selection               the selected text
    read BUFFER [FROM TO]   lines FROM to TO of BUFFER, unsaved changes included
    open FILE [LINE [COL]]  show FILE to the human, the cursor at LINE and COL
    replace FILE OLD NEW    replace the only match of OLD in FILE with NEW, then save
    create FILE TEXT        create FILE with TEXT, then save

`context` prints one JSON object, where the human is:

    buffer      the current BUFFER
    file        its FILE, or null if it has none
    line, col   the cursor's LINE and COL
    mode        what the human's keys do: insert types text, normal runs
                commands, visual extends a selection, prompt types into a
                prompt (a command line, a minibuffer); a modeless editor
                is in insert or prompt
    selection   the primary selection, or null: start and end as
                [LINE, COL], end exclusive; type char, line (COL ignored)
                or block; active false for the last selection, no longer
                shown
    buffers     open buffers, each with buffer, file, modified (unsaved
                changes) and visible (shown in a window)

```json
{
  "buffer": "/src/main.c", "file": "/src/main.c",
  "line": 42, "col": 7, "mode": "normal",
  "selection": {"start": [40, 1], "end": [42, 8], "type": "char", "active": false},
  "buffers": [
    {"buffer": "/src/main.c", "file": "/src/main.c", "modified": false, "visible": true},
    {"buffer": "fugitive:///src/.git//4a2f", "file": null, "modified": false, "visible": false}
  ]
}
```

Rules:

- Edits never move the human's cursor or view. A change the human can see
  appears in place; any other change is made in the background. Only `open`
  moves the view.
- The handler never asks the human anything.

Exit status:

- 0: done
- 1: failed, the reason is on stderr
- 3: the human has unsaved changes in `FILE`; ask them, don't retry
