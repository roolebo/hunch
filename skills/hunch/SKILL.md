---
name: hunch
description: Work with the user's editor registered as a participant. See
  what they see, change their files through it, present larger changes for
  review. Use when the user refers to their editor or what they're looking
  at, and before changing files while an editor is registered.
---

# hunch

The user's editor is a participant in the environment; PROTOCOL.md, next
to this file, specifies its commands. Nothing reaches the user's files
unseen: tiny edits happen live in their editor, everything else is reviewed
first.

`hunch-editor` comes with hunch and is on PATH.

## Changing files

A tiny change the user asks for (one spot, e.g. "rename this", "fix that
line") goes in place through the editor:

- Run `hunch-editor find` first. If it fails, no editor is registered: tell
  the user, then use your normal file tools.
- Use `replace` and `create`. Never change the user's files with your
  built-in write/edit tools, `sed -i` or redirection.

Everything else is reviewed before it reaches the user's files:

1. Check the user's working copy. If anything is uncommitted, stop and ask
   them to commit or set it aside.
2. Work in a separate working copy on a new branch (git: `git worktree add
   -b NAME ../REPO-NAME`), with your normal tools.
3. Commit the work as a series of small commits, one change each, so the
   user can review them one by one.
4. From the user's working copy, show it: `hunch-editor review RANGE` (git:
   `main..NAME`). Fix what they ask for, then show it again; do the same
   after any other rewrite of the series (git: amend, rebase, squash of
   fixups).
5. Once approved, apply the series as is, without merge commits (git:
   `git merge --ff-only NAME`), run `hunch-editor reload`, then remove the
   separate copy. If it doesn't apply, rebase it and review again.

Reading files with your normal tools is fine. Use `read` when the user may
have unsaved changes. Exit status 3 means unsaved changes in the editor:
stop and ask the user to save or discard them.

## Commands

`hunch-editor --help` lists the commands; PROTOCOL.md specifies them.
What the user sees:

    hunch-editor read "$(hunch-editor context | jq -r .buffer)"

Use `open` only when the user asks to see something: it moves their
view.

## Edit

Replace text, like an edit tool. Old text goes on fd 3, new text on fd 4;
it may start or end mid-line. Quoted heredoc delimiters keep the text
literal:

    hunch-editor replace path/to/file 3<<'OLD' 4<<'NEW'
    def Clear(name: string, value: string)
    OLD
    def Clear()
    NEW

The final newline of each heredoc is dropped. To delete lines, include a
neighbouring line in the old text and keep it in the new text.

If the editor doesn't answer (e.g. `Send expression failed`), it may be
suspended or busy: tell the user. Don't make the change with your own tools
instead.
