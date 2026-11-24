//! TREE.md's Serve: a device that serves itself, as Vim does, answers the
//! commands its files take, and is gone once its connection closes.

mod common;
mod conform;

use hunch::device;
use hunch::p9::errno;
use hunch::srv::Srv;
use hunch::tree::Workspace;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

fn dir(name: &str) -> PathBuf {
  let dir = std::env::temp_dir()
    .join(format!("hunch-srv-{}-{name}", std::process::id()));
  std::fs::create_dir_all(&dir).unwrap();
  dir
}

/// Srv at a socket of its own, and the socket
fn srv(name: &str) -> (Arc<Srv>, PathBuf) {
  let socket = dir(name).join("srv.sock");
  let _ = std::fs::remove_file(&socket);
  let listener = UnixListener::bind(&socket).unwrap();
  let srv = Arc::new(Srv::default());
  let served = srv.clone();
  thread::spawn(move || served.serve(listener));
  (srv, socket)
}

/// An editor that serves ACTIVITY, as hunch-9p serve hands it over: it answers context with where its cursor is, until its
/// connection is dropped
fn editor(socket: &PathBuf, activity: &str, file: &str) -> UnixStream {
  let stream = UnixStream::connect(socket).unwrap();
  let header = json!({"activity": activity, "kind": "editor"});
  writeln!(&stream, "{header}").unwrap();
  let answering = stream.try_clone().unwrap();
  let file = file.to_string();
  thread::spawn(move || {
    let mut out = answering.try_clone().unwrap();
    for line in BufReader::new(answering).lines() {
      let Ok(line) = line else { return };
      let asked: Value = serde_json::from_str(&line).unwrap();
      let answer = match asked["command"].as_str() {
        Some("context") => {
          let context = json!({"buffer": file, "file": file, "buffers": []});
          json!({"id": asked["id"], "result": context.to_string()})
        }
        _ => json!({"id": asked["id"], "error": "unsaved", "status": 3}),
      };
      if writeln!(out, "{answer}").is_err() {
        return;
      }
    }
  });
  stream
}

fn workspace(srv: Arc<Srv>, name: &str) -> Arc<Workspace> {
  let editor = device::editor(srv, dir(name));
  let seat = Workspace::new(vec![("editor", Box::new(editor))]);
  Arc::new(Workspace::new(vec![("seat", Box::new(seat))]))
}

/// Waits for what srv's thread does with a connection
fn settle() {
  thread::sleep(std::time::Duration::from_millis(100));
}

#[test]
fn an_editor_that_serves_answers_through_its_files() {
  let (srv, socket) = srv("answers");
  let _vim = editor(&socket, "@1", "/src/a.c");
  settle();
  let mut c = common::attach(workspace(srv, "answers"), "@1");
  let context: Value =
    serde_json::from_slice(&c.read("seat/editor/context").unwrap()).unwrap();
  assert_eq!(context["file"], "/src/a.c");
  // The device's exit status 3 is EBUSY
  let e = c.write("seat/editor/ctl", b"reload\n").unwrap_err();
  assert_eq!(e.errno, errno::EBUSY);
}

#[test]
fn each_activity_sees_its_own_editor() {
  let (srv, socket) = srv("own");
  let _one = editor(&socket, "@1", "/src/one.c");
  let _two = editor(&socket, "@2", "/src/two.c");
  settle();
  let w = workspace(srv, "own");
  for (activity, file) in [("@1", "/src/one.c"), ("@2", "/src/two.c")] {
    let mut c = common::attach(w.clone(), activity);
    let context = c.read("seat/editor/context").unwrap();
    assert!(String::from_utf8_lossy(&context).contains(file));
  }
  let mut none = common::attach(w, "@3");
  let e = none.read("seat/editor/context").unwrap_err();
  assert_eq!(e.errno, errno::ENOENT);
}

#[test]
fn an_editor_that_goes_is_gone() {
  let (srv, socket) = srv("gone");
  let vim = editor(&socket, "@1", "/src/a.c");
  settle();
  let w = workspace(srv, "gone");
  common::attach(w.clone(), "@1")
    .read("seat/editor/context")
    .unwrap();
  vim.shutdown(std::net::Shutdown::Both).unwrap();
  settle();
  let e = common::attach(w, "@1")
    .read("seat/editor/context")
    .unwrap_err();
  assert_eq!(e.errno, errno::ENOENT);
}

#[test]
fn an_editor_that_serves_conforms() {
  let (srv, socket) = srv("conforms");
  let _vim = editor(&socket, "@1", "/src/a.c");
  settle();
  conform::conforms(&mut common::attach(workspace(srv, "conforms"), "@1"));
}
