//! The devices that serve themselves, as TREE.md's Serve says, as Plan 9
//! posts its servers in /srv. hunch-9p serve hands a device's stdin and
//! stdout to hunch-host at ROOT/sock/srv.sock, after a line of JSON with
//! its activity and kind; the workspace has the device while the
//! connection is open.

use crate::p9::errno;
use crate::peer::Peer;
use crate::registry::{Handler, Registry, UNSAVED};
use crate::tree::{Error, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::{BufRead, BufReader};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;

/// Where a device serves: its activity and its kind, e.g. editor
type Point = (String, String);

#[derive(Default)]
pub struct Srv {
  table: Mutex<HashMap<Point, Arc<Peer>>>,
}

impl Srv {
  fn table(&self) -> MutexGuard<'_, HashMap<Point, Arc<Peer>>> {
    self.table.lock().unwrap_or_else(|e| e.into_inner())
  }

  /// The device of KIND that serves ACTIVITY
  pub fn peer(&self, activity: &str, kind: &str) -> Option<Arc<Peer>> {
    let point = (activity.to_string(), kind.to_string());
    self.table().get(&point).cloned()
  }

  /// Takes the devices that serve on LISTENER, each on its own thread
  pub fn serve(self: Arc<Self>, listener: UnixListener) {
    for stream in listener.incoming() {
      let Ok(stream) = stream else { continue };
      let srv = self.clone();
      thread::spawn(move || {
        if let Err(e) = srv.take(stream) {
          eprintln!("hunch-host: a device that serves: {e}");
        }
      });
    }
  }

  /// Takes the device on STREAM, in place of the one of its kind before,
  /// until it closes
  fn take(&self, stream: UnixStream) -> std::io::Result<()> {
    let mut lines = BufReader::new(stream.try_clone()?).lines();
    let header: Value = match lines.next() {
      Some(line) => serde_json::from_str(&line?)?,
      None => return Ok(()),
    };
    let (Some(activity), Some(kind)) =
      (header["activity"].as_str(), header["kind"].as_str())
    else {
      return Err(std::io::Error::other(
        "a device serves with an activity and a kind",
      ));
    };
    let point = (activity.to_string(), kind.to_string());
    let peer = Peer::new(kind, Box::new(stream));
    self.table().insert(point.clone(), peer.clone());
    for line in lines {
      match serde_json::from_str(&line?) {
        Ok(message) => peer.answer(&message),
        Err(e) => eprintln!("hunch-host: {kind} sent a line not JSON: {e}"),
      }
    }
    peer.close();
    // Not a device of the same kind that came since
    let mut table = self.table();
    if table.get(&point).is_some_and(|p| Arc::ptr_eq(p, &peer)) {
      table.remove(&point);
    }
    Ok(())
  }
}

/// A device that serves, as a handler of its protocol; its address is its
/// activity and kind, so the devices of two activities are apart
struct Served {
  peer: Arc<Peer>,
  addr: String,
}

impl Handler for Served {
  fn run(&self, args: &[&OsStr]) -> Result<Vec<u8>> {
    let args: Vec<String> =
      args.iter().map(|a| a.to_string_lossy().into()).collect();
    let (command, rest) = args
      .split_first()
      .ok_or_else(|| Error::new(errno::EINVAL, "no command"))?;
    let rest: Vec<&str> = rest.iter().map(String::as_str).collect();
    match self.peer.ask(command, &rest, None) {
      Ok(Value::String(text)) => Ok(text.into_bytes()),
      Ok(Value::Null) => Ok(Vec::new()),
      Ok(result) => Ok(result.to_string().into_bytes()),
      Err(failure) => Err(Error::new(
        match failure.status {
          Some(status) if status == u64::from(UNSAVED) => errno::EBUSY,
          _ => errno::EIO,
        },
        failure.message,
      )),
    }
  }

  fn addr(&self) -> &str {
    &self.addr
  }
}

impl Registry for Srv {
  /// The device of ROLE that serves ACTIVITY
  fn find(
    &self,
    role: &str,
    activity: Option<&str>,
  ) -> Option<Box<dyn Handler>> {
    let peer = self.peer(activity?, role)?;
    let addr = format!("{} {role}", activity?);
    Some(Box::new(Served { peer, addr }))
  }
}
