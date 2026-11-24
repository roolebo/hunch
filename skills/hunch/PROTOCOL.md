# Editor protocol

The editor is the program the human edits in: Vim, or any other. Agents
reach it through `seat/editor/` in their activity's workspace (see
Files); the editor serves itself to the workspace and answers the
commands below, as TREE.md's Serve says, so the same files work with any
editor.

Terms:

    FILE        a file, by its absolute path on the editor's host
    BUFFER      the editor's BUFFER, as TREE.md says; a file's buffer has
                its FILE as the id
    LINE, COL   a position, from 1; COL counts characters, not bytes or
                screen columns
    OLD, NEW, TEXT
                text, passed in a file, never on the command line
    REPO        a working copy under version control, by its absolute path
    RANGE       the changes to show, in the notation of REPO's version
                control, e.g. main..topic in git

Commands:

    context                 where the human is: file, cursor, selection, open files
    selection               the selected text
    read BUFFER [FROM TO]   lines FROM to TO of BUFFER, unsaved changes included
    open FILE [LINE [COL]]  show FILE to the human, the cursor at LINE and COL
    replace FILE OLD NEW    replace the only match of OLD in FILE with NEW, then save
    create FILE TEXT        create FILE with TEXT, then save
    review REPO RANGE       show the changes of RANGE in REPO to the human
    reload                  re-read files changed on disk; unsaved ones are left alone

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
  and `review` move the view.
- The editor never asks the human anything.

Exit status:

- 0: done
- 1: failed, the reason is on stderr
- 3: the human has unsaved changes in `FILE`; ask them, don't retry

## Files

`seat/editor/` in an activity's workspace (TREE.md) is the editor
that serves the activity. The files follow Plan 9's acme, an editor that
serves itself as files. They are TREE.md's Buffers, context and
selection as the commands print them and body as read gives the whole
buffer, and these:

    seat/editor/
      ctl           write a command and its arguments, one per line:
                    open, reload or review
      new           write BUFFER, then read: its N
      N/
        addr        write OLD
        data        write NEW: replace, OLD from addr; without addr,
                    create; a replace drops one final newline of
                    both

Errors:

    ENOENT      no editor in the activity, or no such N
    EBUSY       exit status 3
    EINVAL      ctl: no such command, or wrong arguments
    EIO         exit status 1; errstr says why
