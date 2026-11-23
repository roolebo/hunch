//! What every tree obeys, as TREE.md's Reads and writes say, checked
//! through the files as an agent sees them: a device that passes is one
//! of the tree's.

use hunch::client::Client;
use hunch::p9::errno;
use hunch::tree::Kind;

/// Walks the whole tree C is attached to, and fails at the first file
/// that breaks a rule
pub fn conforms(c: &mut Client) {
  dir(c, "");
}

fn join(dir: &str, name: &str) -> String {
  if dir.is_empty() {
    name.to_string()
  } else {
    format!("{dir}/{name}")
  }
}

fn dir(c: &mut Client, path: &str) {
  let names = c.ls(path).unwrap_or_else(|e| panic!("{path}/: {e:?}"));
  for name in names {
    let (name, listed_dir) = match name.strip_suffix('/') {
      Some(name) => (name.to_string(), true),
      None => (name, false),
    };
    let child = join(path, &name);
    let kind = c.kind(&child).unwrap_or_else(|e| panic!("{child}: {e:?}"));
    assert_eq!(
      kind == Kind::Dir,
      listed_dir,
      "{child}: listed as a {}, is {kind:?}",
      if listed_dir { "directory" } else { "file" }
    );
    match kind {
      Kind::Dir => dir(c, &child),
      Kind::File { read, write } => file(c, &child, read, write),
    }
  }
  // A name the directory doesn't list isn't there
  let none = join(path, "no-such-name");
  assert_eq!(
    c.kind(&none).map_err(|e| e.errno),
    Err(errno::ENOENT),
    "{none}"
  );
}

fn file(c: &mut Client, path: &str, read: bool, write: bool) {
  // Nothing is under a file
  let under = format!("{path}/x");
  assert!(c.kind(&under).is_err(), "{under} is there");
  if !write {
    let e = c.write(path, b"").unwrap_err();
    assert_eq!(e.errno, errno::EACCES, "{path} took a write");
  }
  if !read {
    let e = c.read(path).unwrap_err();
    assert_eq!(e.errno, errno::EACCES, "{path} gave a read");
  }
  // An observation changes nothing: read twice, it's the same. A file
  // that answers takes a write first, which is an action
  if read && !write {
    let first = c.read(path).map_err(|e| e.errno);
    let second = c.read(path).map_err(|e| e.errno);
    assert_eq!(first, second, "{path} changed when read");
  }
}
