# TODO

- Paths relative to the project root instead of absolute. The editor's
  current directory can't be the base: Vim has one per tab and window. The
  project root can: a VS Code workspace folder, Emacs `project-root`, the
  git root in Vim. `context` would report `root`, paths would be relative to
  it, and an agent on another host would map `root` to its own checkout.
  Likely in 0.2, together with network addresses.

- Explicit editor and agent pairs. find takes the nearest participant by
  tmux window, then session; a group property would pair them when the
  layout doesn't, e.g. two agents in one window.

- Version check. An editor would register `protocol` with the tribe version
  it implements, and tribe-editor would refuse a different minor version. Worth
  it once there's more than one editor.

- Dictation to the agent from a modeless editor. tmux-tribe sends a paste to
  the agent only in normal or visual mode; Emacs and VS Code are always in
  insert, so their pastes always stay in the editor. They need another
  signal.

- Only git is supported: tribe-editor finds REPO and vim-tribe shows RANGE
  with git, and guard knows the agent's separate working copy only as a git
  linked worktree. Other version control, e.g. Perforce, needs its own way
  in each.
