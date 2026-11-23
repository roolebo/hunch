//! hunch-browser-native, run as Firefox runs it: the test is the browser's
//! extension on its pipes, and a client of the socket it serves.

use hunch::client::Client;
use hunch::native::{receive, send};
use serde_json::{Value, json};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// A runtime directory of its own, short for a socket's path
fn runtime(name: &str) -> PathBuf {
  let dir = PathBuf::from("/tmp")
    .join(format!("hunch-native-{}-{name}", std::process::id()));
  let _ = std::fs::remove_dir_all(&dir);
  std::fs::create_dir_all(&dir).unwrap();
  dir
}

/// The bridge, with an extension that answers each command with ANSWER;
/// what the extension says besides goes on the sender, and None closes
/// the pipe, as Firefox does when the extension goes
fn bridge(
  dir: &Path,
  answer: fn(&Value) -> Value,
) -> (Child, mpsc::Sender<Option<Value>>) {
  let mut child = Command::new(env!("CARGO_BIN_EXE_hunch-browser-native"))
    .args(["/manifest/hunch.json", "firefox-hunch@hunch"])
    .env("XDG_RUNTIME_DIR", dir)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .spawn()
    .unwrap();
  let mut stdin = child.stdin.take().unwrap();
  let mut stdout = child.stdout.take().unwrap();
  let (says, said) = mpsc::channel::<Option<Value>>();
  says
    .send(Some(json!({"hello": {"name": "Firefox"}})))
    .unwrap();
  // One thread writes to the bridge, so dropping it closes the pipe
  thread::spawn(move || {
    while let Ok(Some(message)) = said.recv() {
      send(&mut stdin, &message).unwrap();
    }
  });
  let replies = says.clone();
  thread::spawn(move || {
    while let Ok(Some(command)) = receive(&mut stdout) {
      let _ = replies.send(Some(answer(&command)));
    }
  });
  (child, says)
}

fn connect(socket: &Path) -> Client {
  for _ in 0..50 {
    if let Ok(stream) = UnixStream::connect(socket) {
      // Any activity's: the browser belongs to none
      return Client::attach(stream, "@1").unwrap();
    }
    thread::sleep(Duration::from_millis(100));
  }
  panic!("nothing serves {}", socket.display());
}

/// What the human sees in the browser: its active tab's page
fn body(c: &mut Client) -> Vec<u8> {
  let index = String::from_utf8(c.read("index").unwrap()).unwrap();
  let n = index.split('\t').next().unwrap();
  c.read(&format!("{n}/body")).unwrap()
}

fn page(command: &Value) -> Value {
  match command["command"].as_str() {
    Some("context") => json!({"id": command["id"], "result": {"tab": 1}}),
    Some("read") => json!({"id": command["id"], "result": "The page"}),
    Some("dom") => json!({"id": command["id"], "result": command["args"]}),
    _ => json!({"id": command["id"], "error": "No such command"}),
  }
}

#[test]
fn the_browser_is_served_while_firefox_runs_the_bridge() {
  let dir = runtime("served");
  let socket = dir.join("hunch/sock/browser.sock");
  let (mut child, extension) = bridge(&dir, page);
  let mut c = connect(&socket);
  assert_eq!(body(&mut c), b"The page\n");
  assert_eq!(c.rdwr("1/dom", b"#main").unwrap(), b"[\"#main\"]\n");
  let e = c.read("selection").unwrap_err();
  assert_eq!(e.message, "No such command");
  extension.send(None).unwrap();
  assert!(child.wait().unwrap().success());
  assert!(!socket.exists(), "the socket outlived the bridge");
  let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_browser_used_last_takes_the_socket() {
  let dir = runtime("focus");
  let socket = dir.join("hunch/sock/browser.sock");
  let (mut first, focus) = bridge(&dir, page);
  connect(&socket);
  let (mut second, last) = bridge(&dir, |c| match c["command"].as_str() {
    Some("context") => json!({"id": c["id"], "result": {"tab": 2}}),
    _ => json!({"id": c["id"], "result": "The other page"}),
  });
  // The second took the socket; focus gives it back to the first
  thread::sleep(Duration::from_millis(300));
  assert_eq!(body(&mut connect(&socket)), b"The other page\n");
  focus.send(Some(json!({"focus": true}))).unwrap();
  thread::sleep(Duration::from_millis(300));
  assert_eq!(body(&mut connect(&socket)), b"The page\n");
  // The second, gone, leaves the first's socket alone
  last.send(None).unwrap();
  second.wait().unwrap();
  assert_eq!(body(&mut connect(&socket)), b"The page\n");
  focus.send(None).unwrap();
  first.wait().unwrap();
  let _ = std::fs::remove_dir_all(&dir);
}
