//! Laws of assembly, as TREE.md says: what a combinator makes behaves as
//! the devices it's made of, observed through the files.

mod common;
mod conform;

use hunch::tree::{
  ANSWER, ByActivity, Current, Focus, Kind, Remote, Result, Tree, Workspace,
  no_such,
};
use std::sync::{Arc, Mutex};

/// note holds what was last written to it
#[derive(Default)]
struct Note(Mutex<Vec<u8>>);

impl Tree for Note {
  fn kind(&self, _activity: &str, path: &[&str]) -> Result<Kind> {
    match path {
      [] => Ok(Kind::Dir),
      ["note"] => Ok(ANSWER),
      _ => Err(no_such()),
    }
  }

  fn list(
    &self,
    _activity: &str,
    path: &[&str],
  ) -> Result<Vec<(String, Kind)>> {
    match path {
      [] => Ok(vec![("note".into(), ANSWER)]),
      _ => Err(no_such()),
    }
  }

  fn read(
    &self,
    _activity: &str,
    path: &[&str],
    _input: Option<&[u8]>,
  ) -> Result<Vec<u8>> {
    match path {
      ["note"] => Ok(self.0.lock().unwrap().clone()),
      _ => Err(no_such()),
    }
  }

  fn write(&self, _activity: &str, path: &[&str], data: &[u8]) -> Result<()> {
    match path {
      ["note"] => {
        *self.0.lock().unwrap() = data.to_vec();
        Ok(())
      }
      _ => Err(no_such()),
    }
  }
}

/// What an agent does, by path under the device
enum Step {
  Kind(&'static [&'static str]),
  List(&'static [&'static str]),
  Read(&'static [&'static str]),
  Write(&'static [&'static str], &'static [u8]),
}

const STEPS: &[Step] = &[
  Step::Kind(&[]),
  Step::List(&[]),
  Step::Kind(&["note"]),
  Step::Read(&["note"]),
  Step::Write(&["note"], b"hi"),
  Step::Read(&["note"]),
  Step::List(&["note"]),
  Step::Read(&["none"]),
  Step::Write(&["none"], b"hi"),
];

/// What TREE answers to STEPS under UNDER in ACTIVITY; an error by its errno
/// alone, since its words may name what refused
fn observe(
  tree: &dyn Tree,
  activity: &str,
  under: &[&str],
) -> Vec<std::result::Result<String, u32>> {
  STEPS
    .iter()
    .map(|step| {
      let at = |path: &[&'static str]| [under, path].concat();
      let seen = match step {
        Step::Kind(p) => tree.kind(activity, &at(p)).map(|k| format!("{k:?}")),
        Step::List(p) => tree.list(activity, &at(p)).map(|l| format!("{l:?}")),
        Step::Read(p) => tree
          .read(activity, &at(p), None)
          .map(|d| String::from_utf8_lossy(&d).into_owned()),
        Step::Write(p, d) => {
          tree.write(activity, &at(p), d).map(|()| String::new())
        }
      };
      seen.map_err(|e| e.errno)
    })
    .collect()
}

#[test]
fn a_workspace_of_one_device_is_the_device_under_its_name() {
  let workspace = Workspace::new(vec![("x", Box::new(Note::default()))]);
  assert_eq!(
    observe(&workspace, "@1", &["x"]),
    observe(&Note::default(), "@1", &[])
  );
  assert_eq!(
    workspace.list("@1", &[]),
    Ok(vec![("x".to_string(), Kind::Dir)])
  );
}

/// The human in an activity, or in none
struct In(Option<&'static str>);

impl Focus for In {
  fn current(&self) -> Option<String> {
    self.0.map(String::from)
  }
}

#[test]
fn a_device_in_the_current_task_is_the_device() {
  let current = Current::new(Note::default(), Arc::new(In(Some("@1"))));
  assert_eq!(
    observe(&current, "@1", &[]),
    observe(&Note::default(), "@1", &[])
  );
}

#[test]
fn a_device_outside_the_current_task_is_no_device() {
  for focus in [In(Some("@2")), In(None)] {
    let with = Workspace::new(vec![(
      "x",
      Box::new(Current::new(Note::default(), Arc::new(focus))),
    )]);
    let without = Workspace::new(vec![]);
    assert_eq!(
      observe(&with, "@1", &["x"]),
      observe(&without, "@1", &["x"])
    );
    assert_eq!(with.list("@1", &[]), without.list("@1", &[]));
  }
}

#[test]
fn an_attach_without_a_task_is_the_hosts() {
  let activities = ByActivity::new(
    Box::new(Workspace::new(vec![("x", Box::new(Note::default()))])),
    Box::new(Workspace::new(vec![])),
  );
  let host = Workspace::new(vec![("x", Box::new(Note::default()))]);
  assert_eq!(observe(&activities, "", &["x"]), observe(&host, "", &["x"]));
  assert_eq!(
    observe(&activities, "@1", &["x"]),
    observe(&Workspace::new(vec![]), "@1", &["x"])
  );
}

/// Serves TREE at a socket of its own, as hunch-browser-native does
fn served(tree: Note, name: &str) -> std::path::PathBuf {
  let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
  let socket = dir.join(name);
  let _ = std::fs::remove_file(&socket);
  let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
  std::thread::spawn(move || hunch::host::serve(listener, Arc::new(tree), 0));
  socket
}

#[test]
fn a_device_through_a_proxy_is_the_device() {
  let remote = Remote::new(served(Note::default(), "note.sock"), "gone");
  assert_eq!(
    observe(&remote, "@1", &[]),
    observe(&Note::default(), "@1", &[])
  );
}

#[test]
fn a_device_through_a_proxy_obeys_the_tree() {
  let remote = Remote::new(served(Note::default(), "conform.sock"), "gone");
  let workspace = Workspace::new(vec![("x", Box::new(remote))]);
  conform::conforms(&mut common::attach(Arc::new(workspace), "@1"));
}

#[test]
fn a_proxy_to_nothing_is_no_device() {
  let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
  let with = Workspace::new(vec![(
    "x",
    Box::new(Remote::new(dir.join("nothing.sock"), "gone")),
  )]);
  let without = Workspace::new(vec![]);
  assert_eq!(
    observe(&with, "@1", &["x"]),
    observe(&without, "@1", &["x"])
  );
  assert_eq!(with.list("@1", &[]), without.list("@1", &[]));
}
