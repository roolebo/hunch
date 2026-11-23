//! What reads and writes mean, as TREE.md says, through 9P2000.L on a
//! tree in memory.

mod common;
mod conform;

use hunch::p9::errno;
use hunch::tree::{ANSWER, Error, Kind, READ, Result, Tree, WRITE, no_such};
use std::sync::{Arc, Mutex};

/// note holds a text, echo answers what was written to it, log keeps
/// each action, broken fails
#[derive(Default)]
struct Mem {
  actions: Mutex<Vec<Vec<u8>>>,
}

impl Tree for Mem {
  fn kind(&self, _activity: &str, path: &[&str]) -> Result<Kind> {
    match path {
      [] | ["dir"] => Ok(Kind::Dir),
      ["note"] | ["broken"] | ["dir", "inner"] => Ok(READ),
      ["echo"] => Ok(ANSWER),
      ["log"] => Ok(WRITE),
      _ => Err(no_such()),
    }
  }

  fn list(
    &self,
    _activity: &str,
    path: &[&str],
  ) -> Result<Vec<(String, Kind)>> {
    match path {
      [] => Ok(vec![
        ("note".into(), READ),
        ("dir".into(), Kind::Dir),
        ("log".into(), WRITE),
      ]),
      ["dir"] => Ok(vec![("inner".into(), READ)]),
      _ => Err(no_such()),
    }
  }

  fn read(
    &self,
    activity: &str,
    path: &[&str],
    input: Option<&[u8]>,
  ) -> Result<Vec<u8>> {
    match path {
      ["note"] => Ok(format!("note of {activity}\n").into_bytes()),
      ["echo"] => Ok([b"echo: ", input.unwrap_or(b"nothing")].concat()),
      ["broken"] => Err(Error::new(errno::EIO, "broken on purpose")),
      _ => Err(no_such()),
    }
  }

  fn write(&self, _activity: &str, _path: &[&str], data: &[u8]) -> Result<()> {
    self.actions.lock().unwrap().push(data.to_vec());
    Ok(())
  }
}

fn mem() -> Arc<Mem> {
  Arc::new(Mem::default())
}

#[test]
fn an_observation_is_the_tasks() {
  let mut c = common::attach(mem(), "@1");
  assert_eq!(c.read("note").unwrap(), b"note of @1\n");
}

#[test]
fn a_file_that_answers_answers_what_was_written() {
  let mut c = common::attach(mem(), "@1");
  assert_eq!(c.rdwr("echo", b"hi").unwrap(), b"echo: hi");
  assert_eq!(c.read("echo").unwrap(), b"echo: nothing");
}

#[test]
fn an_action_runs_once_when_the_file_is_closed() {
  let m = mem();
  let mut c = common::attach(m.clone(), "@1");
  c.write("log", b"first").unwrap();
  c.write("log", b"").unwrap();
  assert_eq!(*m.actions.lock().unwrap(), [b"first".to_vec(), Vec::new()]);
}

#[test]
fn a_long_text_arrives_whole() {
  let m = mem();
  let mut c = common::attach(m.clone(), "@1");
  let long = vec![b'x'; 3 << 20];
  c.write("log", &long).unwrap();
  assert_eq!(m.actions.lock().unwrap()[0], long);
}

#[test]
fn files_are_read_or_written_as_their_kind_allows() {
  let mut c = common::attach(mem(), "@1");
  assert_eq!(c.write("note", b"x").unwrap_err().errno, errno::EACCES);
  assert_eq!(c.read("log").unwrap_err().errno, errno::EACCES);
}

#[test]
fn a_missing_file_isnt_there_at_any_depth() {
  let mut c = common::attach(mem(), "@1");
  assert_eq!(c.read("gone").unwrap_err().errno, errno::ENOENT);
  assert_eq!(c.read("dir/gone").unwrap_err().errno, errno::ENOENT);
  assert_eq!(c.read("dir/../note").unwrap(), b"note of @1\n");
}

#[test]
fn a_listing_starts_with_errstr() {
  let mut c = common::attach(mem(), "@1");
  assert_eq!(c.ls("").unwrap(), ["errstr", "note", "dir/", "log"]);
  assert_eq!(c.ls("dir").unwrap(), ["inner"]);
}

#[test]
fn errstr_says_why_the_last_action_failed() {
  let mut c = common::attach(mem(), "@1");
  assert_eq!(c.read("errstr").unwrap(), b"");
  let e = c.read("broken").unwrap_err();
  assert_eq!(
    (e.errno, e.message.as_str()),
    (errno::EIO, "broken on purpose")
  );
  assert_eq!(
    c.read("errstr").unwrap(),
    b"broken on purpose\n",
    "reading errstr keeps it"
  );
  let _ = c.read("gone");
  assert_eq!(c.read("errstr").unwrap(), b"", "the errno says it all");
}

#[test]
fn errstr_is_the_connections() {
  let m = mem();
  let mut a = common::attach(m.clone(), "@1");
  let mut b = common::attach(m, "@1");
  let _ = a.read("broken");
  assert_eq!(b.read("errstr").unwrap(), b"");
}

#[test]
fn the_tree_in_memory_obeys_the_tree() {
  conform::conforms(&mut common::attach(mem(), "@1"));
}

/// Lists x as a file, but x is a directory
struct Liar;

impl Tree for Liar {
  fn kind(&self, _activity: &str, path: &[&str]) -> Result<Kind> {
    match path {
      [] | ["x"] => Ok(Kind::Dir),
      _ => Err(no_such()),
    }
  }

  fn list(
    &self,
    _activity: &str,
    path: &[&str],
  ) -> Result<Vec<(String, Kind)>> {
    match path {
      [] => Ok(vec![("x".into(), READ)]),
      _ => Ok(vec![]),
    }
  }

  fn read(&self, _: &str, _: &[&str], _: Option<&[u8]>) -> Result<Vec<u8>> {
    Err(no_such())
  }

  fn write(&self, _: &str, _: &[&str], _: &[u8]) -> Result<()> {
    Err(no_such())
  }
}

/// count changes each time it's read
#[derive(Default)]
struct Fickle(Mutex<u32>);

impl Tree for Fickle {
  fn kind(&self, _activity: &str, path: &[&str]) -> Result<Kind> {
    match path {
      [] => Ok(Kind::Dir),
      ["count"] => Ok(READ),
      _ => Err(no_such()),
    }
  }

  fn list(
    &self,
    _activity: &str,
    _path: &[&str],
  ) -> Result<Vec<(String, Kind)>> {
    Ok(vec![("count".into(), READ)])
  }

  fn read(&self, _: &str, _: &[&str], _: Option<&[u8]>) -> Result<Vec<u8>> {
    let mut n = self.0.lock().unwrap();
    *n += 1;
    Ok(n.to_string().into_bytes())
  }

  fn write(&self, _: &str, _: &[&str], _: &[u8]) -> Result<()> {
    Err(no_such())
  }
}

#[test]
#[should_panic(expected = "listed as a file")]
fn a_tree_that_lists_what_it_isnt_fails() {
  conform::conforms(&mut common::attach(Arc::new(Liar), "@1"));
}

#[test]
#[should_panic(expected = "changed when read")]
fn a_tree_that_changes_when_read_fails() {
  conform::conforms(&mut common::attach(Arc::new(Fickle::default()), "@1"));
}
