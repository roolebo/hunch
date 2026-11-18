//! A program that answers the service's commands on pipes it holds: a
//! browser's extension, a seat through its link, GNOME Shell's extension.
//! The service sends each command with an id; the peer replies with it.

use crate::error::{Error, Result, failed};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long a command waits for the peer's reply
const REPLY_WAIT: Duration = Duration::from_secs(30);

/// The result, or the error with its exit status if the peer gave one
type Reply = std::result::Result<Value, (String, Option<u64>)>;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// How a peer frames its messages
pub enum Wire {
    /// Native messaging's: the length in native byte order, then JSON
    Native,
    /// JSON, a line each
    Lines,
}

pub struct Peer {
    pub name: String,
    wire: Wire,
    out: Mutex<File>,
    waiting: Mutex<HashMap<u64, Sender<Reply>>>,
}

/// A message as native messaging frames it
pub fn send_native(out: &mut File, message: &Value) -> io::Result<()> {
    let message = message.to_string();
    let length = u32::try_from(message.len()).map_err(io::Error::other)?;
    out.write_all(&length.to_ne_bytes())?;
    out.write_all(message.as_bytes())?;
    out.flush()
}

impl Peer {
    pub fn new(name: &str, wire: Wire, out: &File) -> Result<Arc<Peer>> {
        let out = out
            .try_clone()
            .map_err(|e| failed(format!("can't keep {name}'s output: {e}")))?;
        Ok(Arc::new(Peer {
            name: name.into(),
            wire,
            out: Mutex::new(out),
            waiting: Mutex::new(HashMap::new()),
        }))
    }

    fn waiting(&self) -> std::sync::MutexGuard<'_, HashMap<u64, Sender<Reply>>> {
        self.waiting.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn send(&self, message: &Value) -> io::Result<()> {
        let mut out = self.out.lock().unwrap_or_else(|e| e.into_inner());
        match self.wire {
            Wire::Native => send_native(&mut out, message),
            Wire::Lines => writeln!(out, "{message}"),
        }
    }

    /// Hands a reply to the command waiting for it, if one still does
    pub fn answer(&self, message: &Value) {
        let Some(id) = message["id"].as_u64() else {
            return;
        };
        let reply = match message.get("error").and_then(Value::as_str) {
            Some(error) => Err((error.to_string(), message["status"].as_u64())),
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

    /// Runs command with args, and input if it takes TEXT, in the peer
    pub fn ask(&self, command: &str, args: &[&str], input: Option<&str>) -> Result<Value> {
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
        match replied.recv_timeout(REPLY_WAIT) {
            Ok(Ok(result)) => Ok(result),
            // The exit statuses of the protocols, as the peer gave them
            Ok(Err((error, status))) => {
                let message = format!("{name}: {error}");
                Err(match status {
                    Some(4) => Error::Gone(message),
                    Some(5) => Error::Cancelled(message),
                    _ => Error::Failed(message),
                })
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(failed(format!("{name} went away"))),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.waiting().remove(&id);
                Err(failed(format!(
                    "{name} didn't answer in {} s",
                    REPLY_WAIT.as_secs()
                )))
            }
        }
    }
}
