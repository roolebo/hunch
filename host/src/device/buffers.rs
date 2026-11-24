//! TREE.md's Buffers: the files of a device that holds BUFFERs, from
//! what its Holder answers, added to the device's own tree; a buffer
//! that holds a device in turn has its files too.

use crate::p9::errno;
use crate::registry::Handler;
use crate::tree::{Error, Kind, READ, Result, Tree, no_such};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const FILES: [(&str, Kind); 3] =
  [("context", READ), ("selection", READ), ("index", READ)];
const BUFFER_FILES: [(&str, Kind); 2] = [("name", READ), ("body", READ)];

/// How a device that holds BUFFERs answers, in its protocol's commands
pub trait Holder: Send + Sync {
  /// The device in ACTIVITY
  fn device(&self, activity: &str) -> Result<Box<dyn Handler>>;

  /// Where the human is, as JSON with buffer and selection
  fn context(&self, device: &dyn Handler) -> Result<Vec<u8>> {
    device.run(&["context".as_ref()])
  }

  fn selection(&self, device: &dyn Handler) -> Result<Vec<u8>> {
    device.run(&["selection".as_ref()])
  }

  /// The BUFFERs it holds open
  fn buffers(&self, device: &dyn Handler) -> Result<Vec<String>>;

  /// What BUFFER holds
  fn body(&self, device: &dyn Handler, buffer: &str) -> Result<Vec<u8>>;

  /// The program of the same kind that runs in BUFFER, if one does, as
  /// tmux may in a window
  fn inner(
    &self,
    _device: &dyn Handler,
    _buffer: &str,
  ) -> Result<Option<Box<dyn Tree>>> {
    Ok(None)
  }
}

/// Where a path is: among the files here, the device's own, or in the
/// device a buffer holds, under the buffer's N
enum Place {
  Here(Kind),
  Own,
  Held(Box<dyn Tree>),
}

/// The numbers the owner gives each device's BUFFERs, from 1, by the
/// device's address, so the devices of two activities are apart
#[derive(Default)]
pub struct Numbers(Mutex<HashMap<String, Vec<String>>>);

impl Numbers {
  pub fn number(&self, device: &dyn Handler, buffer: &str) -> usize {
    let mut all = self.0.lock().unwrap_or_else(|e| e.into_inner());
    let ids = all.entry(device.addr().to_string()).or_default();
    match ids.iter().position(|id| id == buffer) {
      Some(i) => i + 1,
      None => {
        ids.push(buffer.to_string());
        ids.len()
      }
    }
  }

  /// The BUFFER of N, if the owner gave N to one
  pub fn buffer(&self, device: &dyn Handler, n: &str) -> Option<String> {
    let i = n.parse::<usize>().ok()?.checked_sub(1)?;
    let all = self.0.lock().unwrap_or_else(|e| e.into_inner());
    all.get(device.addr())?.get(i).cloned()
  }

  fn count(&self, device: &dyn Handler) -> usize {
    let all = self.0.lock().unwrap_or_else(|e| e.into_inner());
    all.get(device.addr()).map_or(0, Vec::len)
  }
}

pub struct Buffers {
  holder: Arc<dyn Holder>,
  numbers: Arc<Numbers>,
  own: Box<dyn Tree>,
}

impl Buffers {
  /// HOLDER's buffers, numbered by NUMBERS, beside OWN, the device's own
  /// files; OWN may have files under each N too
  pub fn new(
    holder: Arc<dyn Holder>,
    numbers: Arc<Numbers>,
    own: Box<dyn Tree>,
  ) -> Self {
    Buffers {
      holder,
      numbers,
      own,
    }
  }

  /// What PATH is here, None for the device's own files
  fn ours(&self, device: &dyn Handler, path: &[&str]) -> Option<Kind> {
    let numbered = |n| self.numbers.buffer(device, n).is_some();
    let find = |files: &[(&str, Kind)], name| {
      files.iter().find(|(f, _)| f == name).map(|(_, k)| *k)
    };
    match path {
      [] => Some(Kind::Dir),
      [n] if numbered(n) => Some(Kind::Dir),
      [name] => find(&FILES, name),
      [n, name] if numbered(n) => find(&BUFFER_FILES, name),
      _ => None,
    }
  }

  /// The device that runs in the buffer of N
  fn held(
    &self,
    device: &dyn Handler,
    n: &str,
  ) -> Result<Option<Box<dyn Tree>>> {
    match self.numbers.buffer(device, n) {
      Some(id) => self.holder.inner(device, &id),
      None => Ok(None),
    }
  }

  fn place(
    &self,
    activity: &str,
    device: &dyn Handler,
    path: &[&str],
  ) -> Result<Place> {
    if let Some(kind) = self.ours(device, path) {
      return Ok(Place::Here(kind));
    }
    if let [n, _, ..] = path
      && self.own.kind(activity, path).is_err()
      && let Some(held) = self.held(device, n)?
    {
      return Ok(Place::Held(held));
    }
    Ok(Place::Own)
  }

  fn index(&self, device: &dyn Handler) -> Result<Vec<u8>> {
    let mut index = String::new();
    for id in self.holder.buffers(device)? {
      let n = self.numbers.number(device, &id);
      index.push_str(&format!("{n}\t{id}\n"));
    }
    Ok(index.into_bytes())
  }
}

fn entries(files: &[(&str, Kind)]) -> Vec<(String, Kind)> {
  files.iter().map(|(n, k)| (n.to_string(), *k)).collect()
}

impl Tree for Buffers {
  fn kind(&self, activity: &str, path: &[&str]) -> Result<Kind> {
    let device = self.holder.device(activity)?;
    match self.place(activity, device.as_ref(), path)? {
      Place::Here(kind) => Ok(kind),
      Place::Own => self.own.kind(activity, path),
      Place::Held(held) => held.kind(activity, &path[1..]),
    }
  }

  fn list(&self, activity: &str, path: &[&str]) -> Result<Vec<(String, Kind)>> {
    let device = self.holder.device(activity)?;
    let device = device.as_ref();
    let mut entries = match path {
      [] => {
        let count = self.numbers.count(device);
        let mut entries = entries(&FILES);
        entries.extend((1..=count).map(|n| (n.to_string(), Kind::Dir)));
        entries
      }
      [n] if self.numbers.buffer(device, n).is_some() => {
        let mut entries = entries(&BUFFER_FILES);
        if let Some(held) = self.held(device, n)? {
          entries.extend(held.list(activity, &[]).unwrap_or_default());
        }
        entries
      }
      _ => {
        return match self.place(activity, device, path)? {
          Place::Held(held) => held.list(activity, &path[1..]),
          _ => self.own.list(activity, path),
        };
      }
    };
    entries.extend(self.own.list(activity, path).unwrap_or_default());
    Ok(entries)
  }

  fn read(
    &self,
    activity: &str,
    path: &[&str],
    input: Option<&[u8]>,
  ) -> Result<Vec<u8>> {
    let device = self.holder.device(activity)?;
    let device = device.as_ref();
    match self.place(activity, device, path)? {
      Place::Here(_) => {}
      Place::Own => return self.own.read(activity, path, input),
      Place::Held(held) => return held.read(activity, &path[1..], input),
    }
    match path {
      ["context"] => self.holder.context(device),
      ["selection"] => self.holder.selection(device),
      ["index"] => self.index(device),
      [n, file] => {
        let id = self.numbers.buffer(device, n).ok_or_else(no_such)?;
        match *file {
          "name" => Ok(format!("{id}\n").into_bytes()),
          "body" => self.holder.body(device, &id),
          _ => Err(no_such()),
        }
      }
      _ => Err(no_such()),
    }
  }

  fn write(&self, activity: &str, path: &[&str], data: &[u8]) -> Result<()> {
    let device = self.holder.device(activity)?;
    match self.place(activity, device.as_ref(), path)? {
      Place::Here(_) => Err(Error::new(
        errno::EACCES,
        format!("{}: only read it", path.join("/")),
      )),
      Place::Own => self.own.write(activity, path, data),
      Place::Held(held) => held.write(activity, &path[1..], data),
    }
  }
}
