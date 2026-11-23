//! The browser's extension, reached by native messaging, as BROWSER.md's
//! Native messaging says: the browser runs hunch-browser-native, which
//! passes the extension the commands of BROWSER.md.

use crate::p9::errno;
use crate::registry::Handler;
use crate::tree::{Error, Result};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long a command waits for the extension
const REPLY_WAIT: Duration = Duration::from_secs(30);

/// A message: its length in 4 bytes of native byte order, then JSON;
/// None at the end of the input
pub fn receive(r: &mut impl Read) -> io::Result<Option<Value>> {
  let mut length = [0; 4];
  match r.read_exact(&mut length) {
    Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
    read => read?,
  }
  let mut message = vec![0; u32::from_ne_bytes(length) as usize];
  r.read_exact(&mut message)?;
  serde_json::from_slice(&message)
    .map(Some)
    .map_err(io::Error::other)
}

pub fn send(w: &mut impl Write, message: &Value) -> io::Result<()> {
  let message = message.to_string();
  let length = u32::try_from(message.len()).map_err(io::Error::other)?;
  w.write_all(&length.to_ne_bytes())?;
  w.write_all(message.as_bytes())?;
  w.flush()
}

type Reply = std::result::Result<Value, String>;

/// The extension, as a handler of BROWSER.md's commands
pub struct Extension {
  out: Mutex<Box<dyn Write + Send>>,
  waiting: Mutex<HashMap<u64, Sender<Reply>>>,
  next: AtomicU64,
}

impl Extension {
  /// OUT reaches the extension
  pub fn new(out: Box<dyn Write + Send>) -> Arc<Extension> {
    Arc::new(Extension {
      out: Mutex::new(out),
      waiting: Mutex::new(HashMap::new()),
      next: AtomicU64::new(1),
    })
  }

  fn waiting(&self) -> std::sync::MutexGuard<'_, HashMap<u64, Sender<Reply>>> {
    self.waiting.lock().unwrap_or_else(|e| e.into_inner())
  }

  /// Hands MESSAGE to the command it answers; false if it answers none
  pub fn answer(&self, message: &Value) -> bool {
    let Some(id) = message["id"].as_u64() else {
      return false;
    };
    let reply = match message["error"].as_str() {
      Some(error) => Err(error.to_string()),
      None => Ok(message["result"].clone()),
    };
    if let Some(waiting) = self.waiting().remove(&id) {
      let _ = waiting.send(reply);
    }
    true
  }

  /// Tells every waiting command the extension is gone
  pub fn gone(&self) {
    self.waiting().clear();
  }
}

/// A result as BROWSER.md prints it: text as it is, other JSON as JSON
fn printed(result: &Value) -> Vec<u8> {
  match result {
    Value::Null => Vec::new(),
    Value::String(text) => format!("{text}\n").into_bytes(),
    other => format!("{other}\n").into_bytes(),
  }
}

impl Handler for Extension {
  fn run(&self, args: &[&OsStr]) -> Result<Vec<u8>> {
    let args: Vec<String> = args
      .iter()
      .map(|a| a.to_string_lossy().into_owned())
      .collect();
    let (command, args) = args.split_first().expect("a command");
    let id = self.next.fetch_add(1, Ordering::Relaxed);
    let (reply, replied) = mpsc::channel();
    self.waiting().insert(id, reply);
    let message = json!({"id": id, "command": command, "args": args});
    let sent = send(
      &mut *self.out.lock().unwrap_or_else(|e| e.into_inner()),
      &message,
    );
    if let Err(e) = sent {
      self.waiting().remove(&id);
      return Err(Error::new(
        errno::EIO,
        format!("can't reach the browser: {e}"),
      ));
    }
    match replied.recv_timeout(REPLY_WAIT) {
      Ok(Ok(result)) => Ok(printed(&result)),
      Ok(Err(error)) => Err(Error::new(errno::EIO, error)),
      Err(mpsc::RecvTimeoutError::Disconnected) => {
        Err(Error::new(errno::EIO, "the browser went away"))
      }
      Err(mpsc::RecvTimeoutError::Timeout) => {
        self.waiting().remove(&id);
        Err(Error::new(
          errno::EIO,
          format!("the browser didn't answer in {} s", REPLY_WAIT.as_secs()),
        ))
      }
    }
  }

  fn addr(&self) -> &str {
    "native messaging"
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::os::unix::net::UnixStream;
  use std::thread;

  #[test]
  fn a_message_comes_back_as_it_went() {
    let mut buffer = Vec::new();
    send(&mut buffer, &json!({"focus": true})).unwrap();
    let mut r = buffer.as_slice();
    assert_eq!(receive(&mut r).unwrap(), Some(json!({"focus": true})));
    assert_eq!(receive(&mut r).unwrap(), None);
  }

  /// The extension, answering each command with ANSWER
  fn extension(answer: fn(&Value) -> Value) -> Arc<Extension> {
    let (ours, theirs) = UnixStream::pair().unwrap();
    let extension = Extension::new(Box::new(ours.try_clone().unwrap()));
    let answers = extension.clone();
    thread::spawn(move || {
      let (mut r, mut w) = (theirs.try_clone().unwrap(), theirs);
      while let Ok(Some(command)) = receive(&mut r) {
        send(&mut w, &answer(&command)).unwrap();
      }
    });
    thread::spawn(move || {
      let mut r = ours;
      while let Ok(Some(message)) = receive(&mut r) {
        answers.answer(&message);
      }
    });
    extension
  }

  #[test]
  fn a_command_gets_its_result_as_text() {
    let e = extension(|c| {
      let result = format!("{} {}", c["command"], c["args"]);
      json!({"id": c["id"], "result": result})
    });
    let out = e.run(&["dom".as_ref(), "#a".as_ref()]).unwrap();
    assert_eq!(out, b"\"dom\" [\"#a\"]\n");
  }

  #[test]
  fn json_comes_as_json_and_errors_as_eio() {
    let e = extension(|c| match c["command"].as_str() {
      Some("context") => json!({"id": c["id"], "result": {"tab": 7}}),
      _ => json!({"id": c["id"], "error": "No element matches #x"}),
    });
    assert_eq!(e.run(&["context".as_ref()]).unwrap(), b"{\"tab\":7}\n");
    let error = e.run(&["dom".as_ref(), "#x".as_ref()]).unwrap_err();
    assert_eq!(error.errno, errno::EIO);
    assert_eq!(error.message, "No element matches #x");
  }
}
