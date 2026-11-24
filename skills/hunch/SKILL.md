---
name: hunch
description: Work with the user's editor and browser through hunch's
  workspace. See what they see, change their files through the editor,
  present larger changes for review, read what they see in their browser.
  Use when the user refers to their editor, their browser or what they're
  looking at, and before changing files while an editor serves.
---

# hunch

The user's editor and browser are files in your activity's workspace,
which `hunch-9p` reaches; TREE.md, next to this file, says how, and
PROTOCOL.md and BROWSER.md specify the files. Nothing reaches the user's
files unseen: tiny edits happen live in their editor, everything else is
reviewed first.

## Changing files

A tiny change the user asks for (one spot, e.g. "rename this", "fix that
line") goes in place through the editor:

- Run `hunch-9p ls seat/editor` first. If it fails, there is no editor in
  your activity: tell the user, then use your normal file tools.
- Edit through the editor's files, as Edit shows. Never change the
  user's files with your built-in write/edit tools, `sed -i` or
  redirection.

Everything else is reviewed before it reaches the user's files:

1. Check the user's working copy. If anything is uncommitted, stop and ask
   them to commit or set it aside.
2. Work in a separate working copy on a new branch (git: `git worktree add
   -b NAME ../REPO-NAME`), with your normal tools.
3. Commit the work as a series of small commits, one change each, so the
   user can review them one by one.
4. Show it from the user's working copy: `echo "review REPO RANGE" |
   hunch-9p write seat/editor/ctl` (git: `main..NAME`). Fix what they ask
   for, then show it again; do the same after any other rewrite of the
   series (git: amend, rebase, squash of fixups).
5. Once approved, apply the series as is, without merge commits (git:
   `git merge --ff-only NAME`), write `reload` to `seat/editor/ctl`,
   then remove the separate copy. If it doesn't apply, rebase it and
   review again.

Reading files with your normal tools is fine. Read a buffer's `body` when
the user may have unsaved changes. Exit status 3 means unsaved changes in
the editor: stop and ask the user to save or discard them.

## What the user sees

The editor, the browser and the windows hold buffers alike, as TREE.md's
Buffers says: `context` is where the user is, `index` numbers the open
buffers, and `N/body` is what one holds.

    hunch-9p read seat/editor/context

A buffer's text, unsaved changes included:

    n=$(printf %s "$FILE" | hunch-9p rdwr seat/editor/new)
    hunch-9p read "seat/editor/$n/body"

Write `open FILE LINE` to `seat/editor/ctl` only when the user asks to see
something: it moves their view.

## Edit

Replace text, like an edit tool: the old text goes to the buffer's
`addr`, then the new text to its `data`; it may start or end mid-line.
Quoted heredoc delimiters keep the text literal:

    n=$(printf %s path/to/file | hunch-9p rdwr seat/editor/new)
    hunch-9p write "seat/editor/$n/addr" <<'OLD'
    def Clear(name: string, value: string)
    OLD
    hunch-9p write "seat/editor/$n/data" <<'NEW'
    def Clear()
    NEW

A replace drops the final newline of each heredoc. To delete lines,
include a neighbouring line in the old text and keep it in the new text.
`data` without `addr` creates the file, with the text as written.

If the editor doesn't answer, it may be suspended or busy: tell the user.
Don't make the change with your own tools instead.

## The browser

`seat/browser/` is the browser, when the user is in your activity. When
the user points at something in the browser ("this", "here"), it's their
selection: start with `hunch-9p read seat/browser/context`. The page is
the only buffer of `seat/browser/index`.
