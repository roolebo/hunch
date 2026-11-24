//! A program that answers commands on a connection it holds, as TREE.md's
//! Serve says: a device that serves itself, or GNOME Shell's extension on
//! the seat. Each command goes with an id, and the peer replies with it, a
//! JSON object per line.

use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{self, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

/// How long a command waits for the peer's reply
pub const REPLY_WAIT: Duration = Duration::from_secs(30);

/// Why a command failed: the peer's words, and the exit status it gave,
/// none when it gave no answer at all
#[derive(Debug)]
pub struct Failure {
  pub message: String,
  pub status: Option<u64>,
}

type Reply = Result<Value, Failure>;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub struct Peer {
  pub name: String,
  out: Mutex<Box<dyn Write + Send>>,
  waiting: Mutex<HashMap<u64, Sender<Reply>>>,
}

impl Peer {
  /// NAME answers on what reads OUT's other end
  pub fn new(name: &str, out: Box<dyn Write + Send>) -> Arc<Peer> {
    Arc::new(Peer {
      name: name.into(),
      out: Mutex::new(out),
      waiting: Mutex::new(HashMap::new()),
    })
  }

  fn waiting(&self) -> MutexGuard<'_, HashMap<u64, Sender<Reply>>> {
    self.waiting.lock().unwrap_or_else(|e| e.into_inner())
  }

  /// Sends MESSAGE as JSON on a line
  fn send(&self, message: &Value) -> io::Result<()> {
    let mut out = self.out.lock().unwrap_or_else(|e| e.into_inner());
    writeln!(out, "{message}")?;
    out.flush()
  }

  /// Hands a reply to the command waiting for it, if one still does
  pub fn answer(&self, message: &Value) {
    let Some(id) = message["id"].as_u64() else {
      return;
    };
    let reply = match message.get("error").and_then(Value::as_str) {
      Some(error) => Err(Failure {
        message: format!("{}: {error}", self.name),
        status: message["status"].as_u64(),
      }),
      None => Ok(message["result"].clone()),
    };
    if let Some(waiting) = self.waiting().remove(&id) {
      let _ = waiting.send(reply);
    }
  }

  /// Tells each waiting command the peer is gone, by dropping its sender
  pub fn close(&self) {
    self.waiting().clear();
  }

  /// Runs COMMAND with ARGS, and INPUT if it takes text, in the peer
  pub fn ask(
    &self,
    command: &str,
    args: &[&str],
    input: Option<&str>,
  ) -> Reply {
    self.ask_for(REPLY_WAIT, command, args, input)
  }

  /// As ask, waiting up to WAIT, as for the human to pick a window
  pub fn ask_for(
    &self,
    wait: Duration,
    command: &str,
    args: &[&str],
    input: Option<&str>,
  ) -> Reply {
    let failed = |message: String| Failure {
      message,
      status: None,
    };
    let name = &self.name;
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let (reply, replied) = mpsc::channel();
    self.waiting().insert(id, reply);
    let mut message = json!({"id": id, "command": command, "args": args});
    if let Some(input) = input {
      message["input"] = input.into();
    }
    if let Err(e) = self.send(&message) {
      self.waiting().remove(&id);
      return Err(failed(format!("can't reach {name}: {e}")));
    }
    match replied.recv_timeout(wait) {
      Ok(reply) => reply,
      Err(mpsc::RecvTimeoutError::Disconnected) => {
        Err(failed(format!("{name} went away")))
      }
      Err(mpsc::RecvTimeoutError::Timeout) => {
        self.waiting().remove(&id);
        Err(failed(format!(
          "{name} didn't answer in {} s",
          wait.as_secs()
        )))
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::sync::mpsc::Receiver;

  /// What a peer writes, a line at a time
  struct Lines(Sender<String>, String);

  impl Write for Lines {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
      self.1 += &String::from_utf8_lossy(buf);
      while let Some(end) = self.1.find('\n') {
        let line: String = self.1.drain(..=end).collect();
        let _ = self.0.send(line);
      }
      Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
      Ok(())
    }
  }

  fn peer() -> (Arc<Peer>, Receiver<String>) {
    let (sent, lines) = mpsc::channel();
    (
      Peer::new("Vim", Box::new(Lines(sent, String::new()))),
      lines,
    )
  }

  /// Answers the next command PEER sent with REPLY, given its id
  fn reply(peer: &Arc<Peer>, lines: Receiver<String>, reply: Value) {
    let peer = peer.clone();
    std::thread::spawn(move || {
      let sent: Value = serde_json::from_str(&lines.recv().unwrap()).unwrap();
      let mut reply = reply;
      reply["id"] = sent["id"].clone();
      peer.answer(&reply);
    });
  }

  #[test]
  fn a_command_gets_its_reply() {
    let (peer, lines) = peer();
    reply(&peer, lines, json!({"result": "done"}));
    let result = peer.ask("open", &["/src/a.c"], None).unwrap();
    assert_eq!(result, "done");
  }

  #[test]
  fn an_error_keeps_its_status() {
    let (peer, lines) = peer();
    reply(&peer, lines, json!({"error": "unsaved", "status": 3}));
    let e = peer.ask("replace", &[], None).unwrap_err();
    assert_eq!((e.message.as_str(), e.status), ("Vim: unsaved", Some(3)));
  }

  #[test]
  fn a_peer_that_closes_fails_its_commands() {
    let (peer, _lines) = peer();
    let closer = peer.clone();
    std::thread::spawn(move || {
      std::thread::sleep(Duration::from_millis(50));
      closer.close();
    });
    let e = peer.ask("context", &[], None).unwrap_err();
    assert_eq!(e.status, None);
    assert!(e.message.contains("went away"), "{}", e.message);
  }
}
