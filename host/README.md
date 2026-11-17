# hunch-host

Serves what a host has to hunch, as the protocols in
[skills/hunch](../skills/hunch) specify: so far the host's window
managers; its seat and compute come next.

    hunch-host wm ADDR COMMAND [ARGUMENTS]

Window managers:

- tmux, on any Unix: every command but pick

Each role is a Cargo feature, all on by default; a build can leave out
the ones a host doesn't need, e.g. `cargo build --no-default-features
--features wm`.

To test it, with tmux and jq installed:

    cargo test
    test/run
