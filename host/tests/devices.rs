//! TREE.md through 9P2000.L: a client and hunch-host's server on the two
//! ends of a socket, with devices whose handlers are fakes.

mod common;
mod conform;

use hunch::client::Client;
use hunch::device;
use hunch::p9::errno;
use hunch::registry::{Handler, Registry};
use hunch::tree::{Current, Focus};
use hunch::tree::{Error, Result, Workspace};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// What the fake handlers did, each call as its arguments, the texts of
/// files given in place of their paths
type Calls = Arc<Mutex<Vec<Vec<String>>>>;

/// An editor of PROTOCOL.md: FILEs with their lines, and which have
/// unsaved changes
struct FakeEditor {
  files: Mutex<HashMap<String, Vec<String>>>,
  unsaved: Vec<String>,
  calls: Calls,
}

fn text(path: &OsStr) -> String {
  fs::read_to_string(path).expect("the editor gets its texts in files")
}

impl Handler for FakeEditor {
  fn run(&self, args: &[&OsStr]) -> Result<Vec<u8>> {
    let s: Vec<String> = args
      .iter()
      .map(|a| a.to_string_lossy().into_owned())
      .collect();
    let mut files = self.files.lock().unwrap();
    let call = match s[0].as_str() {
      "replace" | "create" => s[..2]
        .iter()
        .cloned()
        .chain(args[2..].iter().map(|a| text(a)))
        .collect(),
      _ => s.clone(),
    };
    self.calls.lock().unwrap().push(call.clone());
    match s[0].as_str() {
      "context" => {
        let buffers: Vec<_> = files
          .keys()
          .map(|f| serde_json::json!({ "buffer": f }))
          .collect();
        Ok(
          serde_json::json!({ "buffers": buffers })
            .to_string()
            .into_bytes(),
        )
      }
      "selection" => Ok(b"selected\n".to_vec()),
      "read" => {
        let lines = files
          .get(&s[1])
          .ok_or_else(|| Error::new(errno::EIO, "not found"))?;
        let numbered: String = lines
          .iter()
          .enumerate()
          .map(|(i, l)| format!("{:6}\t{l}\n", i + 1))
          .collect();
        Ok(numbered.into_bytes())
      }
      "replace" if self.unsaved.contains(&s[1]) => {
        Err(Error::new(errno::EBUSY, "unsaved changes"))
      }
      "replace" => {
        let lines = files
          .get_mut(&s[1])
          .ok_or_else(|| Error::new(errno::EIO, "not found"))?;
        let i = lines
          .iter()
          .position(|l| *l == call[2])
          .ok_or_else(|| Error::new(errno::EIO, "no match"))?;
        lines[i] = call[3].clone();
        Ok(Vec::new())
      }
      "create" => {
        files.insert(s[1].clone(), call[2].lines().map(String::from).collect());
        Ok(Vec::new())
      }
      "open" | "reload" | "review" => Ok(Vec::new()),
      _ => Err(Error::new(errno::EIO, "no such command")),
    }
  }

  fn addr(&self) -> &str {
    "fake-editor"
  }
}

/// Answers each command with its arguments, so a test sees which one ran,
/// save context: a browser of BROWSER.md with tab 7 active
struct FakeBrowser {
  calls: Calls,
}

impl Handler for FakeBrowser {
  fn run(&self, args: &[&OsStr]) -> Result<Vec<u8>> {
    let s: Vec<String> = args
      .iter()
      .map(|a| a.to_string_lossy().into_owned())
      .collect();
    self.calls.lock().unwrap().push(s.clone());
    if s == ["context"] {
      let context =
        serde_json::json!({"tab": 7, "title": "Example", "selection": null});
      return Ok(context.to_string().into_bytes());
    }
    Ok(format!("{}\n", s.join(" ")).into_bytes())
  }

  fn addr(&self) -> &str {
    "fake-browser"
  }
}

/// Activity @1 has an editor, @2 has none; the human is in ACTIVE
struct Fakes {
  editor: Arc<FakeEditor>,
  browser: Arc<FakeBrowser>,
  current: Mutex<Option<String>>,
}

/// So that Registry hands out the same fakes
struct Shared<T: Handler>(Arc<T>);

impl<T: Handler> Handler for Shared<T> {
  fn run(&self, args: &[&OsStr]) -> Result<Vec<u8>> {
    self.0.run(args)
  }
  fn addr(&self) -> &str {
    self.0.addr()
  }
}

impl Registry for Fakes {
  fn find(
    &self,
    role: &str,
    activity: Option<&str>,
  ) -> Option<Box<dyn Handler>> {
    match (role, activity) {
      ("editor", Some("@1")) => Some(Box::new(Shared(self.editor.clone()))),
      ("browser", None) => Some(Box::new(Shared(self.browser.clone()))),
      _ => None,
    }
  }
}

impl Focus for Fakes {
  fn current(&self) -> Option<String> {
    self.current.lock().unwrap().clone()
  }
}

struct Host {
  fakes: Arc<Fakes>,
  workspace: Arc<Workspace>,
  calls: Calls,
}

fn tmp() -> PathBuf {
  let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("texts");
  fs::create_dir_all(&dir).unwrap();
  dir
}

/// As hunch-host assembles its workspace
fn host() -> Host {
  let calls = Calls::default();
  let mut files = HashMap::new();
  files.insert(
    "/src/a.c".to_string(),
    vec!["int a;".to_string(), "int b;".to_string()],
  );
  files.insert("/src/dirty.c".to_string(), vec!["x".to_string()]);
  let fakes = Arc::new(Fakes {
    editor: Arc::new(FakeEditor {
      files: Mutex::new(files),
      unsaved: vec!["/src/dirty.c".to_string()],
      calls: calls.clone(),
    }),
    browser: Arc::new(FakeBrowser {
      calls: calls.clone(),
    }),
    current: Mutex::new(Some("@1".to_string())),
  });
  let workspace = Workspace::new(vec![
    ("editor", Box::new(device::editor(fakes.clone(), tmp()))),
    (
      "browser",
      Box::new(Current::new(device::browser(fakes.clone()), fakes.clone())),
    ),
  ]);
  Host {
    fakes,
    workspace: Arc::new(workspace),
    calls,
  }
}

impl Host {
  fn attach(&self, activity: &str) -> Client {
    common::attach(self.workspace.clone(), activity)
  }

  fn last_call(&self) -> Vec<String> {
    self
      .calls
      .lock()
      .unwrap()
      .last()
      .cloned()
      .unwrap_or_default()
  }
}

fn number(c: &mut Client, buffer: &str) -> String {
  String::from_utf8(c.rdwr("editor/new", buffer.as_bytes()).unwrap())
    .unwrap()
    .trim()
    .to_string()
}

#[test]
fn every_device_obeys_the_tree() {
  let h = host();
  let mut c = h.attach("@1");
  // A buffer, so the editor has one to show
  number(&mut c, "/src/a.c");
  conform::conforms(&mut c);
  conform::conforms(&mut h.attach("@2"));
}

#[test]
fn a_task_sees_only_the_devices_it_has() {
  let h = host();
  assert_eq!(
    h.attach("@1").ls("").unwrap(),
    ["errstr", "editor/", "browser/"]
  );
  assert_eq!(h.attach("@2").ls("").unwrap(), ["errstr"]);
}

#[test]
fn a_failed_observation_says_why_in_errstr() {
  let h = host();
  let mut c = h.attach("@2");
  let e = c.read("editor/context").unwrap_err();
  assert_eq!(e.errno, errno::ENOENT);
  assert!(
    e.message.contains("no editor in activity @2"),
    "{}",
    e.message
  );
}

#[test]
fn context_and_selection_are_the_editors() {
  let h = host();
  let mut c = h.attach("@1");
  assert_eq!(c.read("editor/selection").unwrap(), b"selected\n");
  let context: serde_json::Value =
    serde_json::from_slice(&c.read("editor/context").unwrap()).unwrap();
  assert_eq!(context["buffers"].as_array().unwrap().len(), 2);
}

#[test]
fn a_buffer_is_read_by_its_number() {
  let h = host();
  let mut c = h.attach("@1");
  let n = number(&mut c, "/src/a.c");
  assert_eq!(c.read(&format!("editor/{n}/name")).unwrap(), b"/src/a.c\n");
  assert_eq!(
    c.read(&format!("editor/{n}/body")).unwrap(),
    b"int a;\nint b;\n"
  );
  assert_eq!(number(&mut c, "/src/a.c"), n, "a buffer keeps its number");
  let index = String::from_utf8(c.read("editor/index").unwrap()).unwrap();
  assert!(index.contains(&format!("{n}\t/src/a.c\n")), "{index}");
}

#[test]
fn new_needs_a_buffer() {
  let h = host();
  let e = h.attach("@1").read("editor/new").unwrap_err();
  assert_eq!(e.errno, errno::EINVAL);
}

#[test]
fn addr_then_data_replaces() {
  let h = host();
  let mut c = h.attach("@1");
  let n = number(&mut c, "/src/a.c");
  c.write(&format!("editor/{n}/addr"), b"int b;").unwrap();
  c.write(&format!("editor/{n}/data"), b"long b;").unwrap();
  assert_eq!(h.last_call(), ["replace", "/src/a.c", "int b;", "long b;"]);
  assert_eq!(
    c.read(&format!("editor/{n}/body")).unwrap(),
    b"int a;\nlong b;\n"
  );
}

#[test]
fn data_without_addr_creates() {
  let h = host();
  let mut c = h.attach("@1");
  let n = number(&mut c, "/src/new.c");
  c.write(&format!("editor/{n}/data"), b"one\ntwo\n").unwrap();
  assert_eq!(h.last_call(), ["create", "/src/new.c", "one\ntwo\n"]);
}

#[test]
fn data_takes_what_addr_holds_once() {
  let h = host();
  let mut c = h.attach("@1");
  let n = number(&mut c, "/src/a.c");
  c.write(&format!("editor/{n}/addr"), b"int a;").unwrap();
  c.write(&format!("editor/{n}/data"), b"int z;").unwrap();
  c.write(&format!("editor/{n}/data"), b"again").unwrap();
  assert_eq!(h.last_call()[0], "create");
}

#[test]
fn unsaved_changes_make_data_busy() {
  let h = host();
  let mut c = h.attach("@1");
  let n = number(&mut c, "/src/dirty.c");
  c.write(&format!("editor/{n}/addr"), b"x").unwrap();
  let e = c.write(&format!("editor/{n}/data"), b"y").unwrap_err();
  assert_eq!(
    (e.errno, e.message.as_str()),
    (errno::EBUSY, "unsaved changes")
  );
}

#[test]
fn the_editors_refusal_says_why() {
  let h = host();
  let mut c = h.attach("@1");
  let n = number(&mut c, "/src/a.c");
  c.write(&format!("editor/{n}/addr"), b"missing").unwrap();
  let e = c.write(&format!("editor/{n}/data"), b"y").unwrap_err();
  assert_eq!((e.errno, e.message.as_str()), (errno::EIO, "no match"));
}

#[test]
fn ctl_runs_the_editors_commands() {
  let h = host();
  let mut c = h.attach("@1");
  c.write("editor/ctl", b"open\n/src/a b.c\n2\n").unwrap();
  assert_eq!(h.last_call(), ["open", "/src/a b.c", "2"]);
  c.write("editor/ctl", b"review\n/src\nmain..topic\n")
    .unwrap();
  assert_eq!(h.last_call(), ["review", "/src", "main..topic"]);
  assert_eq!(
    c.write("editor/ctl", b"close\n").unwrap_err().errno,
    errno::EINVAL
  );
}

#[test]
fn an_unknown_buffer_isnt_there() {
  let h = host();
  let e = h.attach("@1").read("editor/9/body").unwrap_err();
  assert_eq!(e.errno, errno::ENOENT);
}

#[test]
fn the_browser_is_in_the_current_task_only() {
  let h = host();
  assert_eq!(h.attach("@1").read("browser/index").unwrap(), b"1\t7\n");
  *h.fakes.current.lock().unwrap() = Some("@2".into());
  let e = h.attach("@1").read("browser/context").unwrap_err();
  assert_eq!(e.errno, errno::ENOENT);
  assert!(e.message.contains("in activity @2"), "{}", e.message);
  h.attach("@2").read("browser/context").unwrap();
}

#[test]
fn the_active_tab_is_the_browsers_buffer() {
  let h = host();
  let mut c = h.attach("@1");
  let context: serde_json::Value =
    serde_json::from_slice(&c.read("browser/context").unwrap()).unwrap();
  assert_eq!(context["buffer"], 7);
  assert!(context.get("tab").is_none(), "{context}");
  assert_eq!(c.read("browser/index").unwrap(), b"1\t7\n");
  assert_eq!(c.read("browser/1/name").unwrap(), b"7\n");
  assert_eq!(c.read("browser/1/body").unwrap(), b"read\n");
}

#[test]
fn dom_answers_for_a_selector() {
  let h = host();
  let mut c = h.attach("@1");
  c.read("browser/index").unwrap();
  assert_eq!(
    c.rdwr("browser/1/dom", b"#main p\n").unwrap(),
    b"dom #main p\n"
  );
  assert_eq!(c.read("browser/1/dom").unwrap(), b"dom\n");
}
