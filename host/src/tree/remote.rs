//! A device another program serves over 9P2000.L at a socket, assembled
//! into a workspace as if hunch-host served it itself, as TREE.md's
//! Assembly says.

use crate::client::Client;
use crate::p9::errno;
use crate::tree::{Error, Kind, Result, Tree};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

pub struct Remote {
  socket: PathBuf,
  /// Why there's no device when nothing serves the socket
  missing: &'static str,
}

impl Remote {
  pub fn new(socket: PathBuf, missing: &'static str) -> Self {
    Remote { socket, missing }
  }

  /// A connection for one operation, so that a device that restarts
  /// is reached again; in ACTIVITY, which a device of no activity ignores
  fn client(&self, activity: &str) -> Result<Client> {
    let stream = UnixStream::connect(&self.socket)
      .map_err(|_| Error::new(errno::ENOENT, self.missing))?;
    Client::attach(stream, activity)
  }
}

impl Tree for Remote {
  fn kind(&self, activity: &str, path: &[&str]) -> Result<Kind> {
    self.client(activity)?.kind(&path.join("/"))
  }

  fn list(&self, activity: &str, path: &[&str]) -> Result<Vec<(String, Kind)>> {
    let mut c = self.client(activity)?;
    let dir = path.join("/");
    let mut entries = Vec::new();
    for name in c.ls(&dir)? {
      let name = name.trim_end_matches('/');
      // errstr is the server's own, not the device's
      if path.is_empty() && name == "errstr" {
        continue;
      }
      let child = if dir.is_empty() {
        name.to_string()
      } else {
        format!("{dir}/{name}")
      };
      entries.push((name.to_string(), c.kind(&child)?));
    }
    Ok(entries)
  }

  fn read(
    &self,
    activity: &str,
    path: &[&str],
    input: Option<&[u8]>,
  ) -> Result<Vec<u8>> {
    let mut c = self.client(activity)?;
    match input {
      Some(input) => c.rdwr(&path.join("/"), input),
      None => c.read(&path.join("/")),
    }
  }

  fn write(&self, activity: &str, path: &[&str], data: &[u8]) -> Result<()> {
    self.client(activity)?.write(&path.join("/"), data)
  }
}
