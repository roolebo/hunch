//! A tree of files, as a device or a workspace serves it; TREE.md says
//! what they are.

use crate::p9::errno;
use std::fmt;

/// Why an observation or an action failed: an errno, and errstr's words
#[derive(Debug, PartialEq)]
pub struct Error {
  pub errno: u32,
  pub message: String,
}

impl Error {
  pub fn new(errno: u32, message: impl Into<String>) -> Self {
    Error {
      errno,
      message: message.into(),
    }
  }
}

impl fmt::Display for Error {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    f.write_str(&self.message)
  }
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
  Dir,
  /// A file that answers is both: a write, then a read of the answer
  File {
    read: bool,
    write: bool,
  },
}

pub const READ: Kind = Kind::File {
  read: true,
  write: false,
};
pub const WRITE: Kind = Kind::File {
  read: false,
  write: true,
};
pub const ANSWER: Kind = Kind::File {
  read: true,
  write: true,
};

/// Paths are names from the tree's top, in the ACTIVITY's workspace
pub trait Tree: Send + Sync {
  fn kind(&self, activity: &str, path: &[&str]) -> Result<Kind>;
  fn list(&self, activity: &str, path: &[&str]) -> Result<Vec<(String, Kind)>>;
  /// What the file holds; for a file that answers, input is what was
  /// written to it first
  fn read(
    &self,
    activity: &str,
    path: &[&str],
    input: Option<&[u8]>,
  ) -> Result<Vec<u8>>;
  fn write(&self, activity: &str, path: &[&str], data: &[u8]) -> Result<()>;
}

/// The errno says it all
pub fn no_such() -> Error {
  Error::new(errno::ENOENT, "")
}

/// Devices side by side, each in its own directory: a workspace
pub struct Workspace {
  devices: Vec<(&'static str, Box<dyn Tree>)>,
}

impl Workspace {
  pub fn new(devices: Vec<(&'static str, Box<dyn Tree>)>) -> Self {
    Workspace { devices }
  }

  fn device<'a>(
    &self,
    path: &'a [&'a str],
  ) -> Result<(&dyn Tree, &'a [&'a str])> {
    let (name, rest) = path.split_first().ok_or_else(no_such)?;
    self
      .devices
      .iter()
      .find(|(n, _)| n == name)
      .map(|(_, d)| (d.as_ref(), rest))
      .ok_or_else(no_such)
  }
}

impl Tree for Workspace {
  fn kind(&self, activity: &str, path: &[&str]) -> Result<Kind> {
    if path.is_empty() {
      return Ok(Kind::Dir);
    }
    let (device, rest) = self.device(path)?;
    device.kind(activity, rest)
  }

  fn list(&self, activity: &str, path: &[&str]) -> Result<Vec<(String, Kind)>> {
    if !path.is_empty() {
      let (device, rest) = self.device(path)?;
      return device.list(activity, rest);
    }
    // Only the devices the activity has
    Ok(
      self
        .devices
        .iter()
        .filter(|(_, d)| d.kind(activity, &[]).is_ok())
        .map(|(name, _)| (name.to_string(), Kind::Dir))
        .collect(),
    )
  }

  fn read(
    &self,
    activity: &str,
    path: &[&str],
    input: Option<&[u8]>,
  ) -> Result<Vec<u8>> {
    let (device, rest) = self.device(path)?;
    device.read(activity, rest, input)
  }

  fn write(&self, activity: &str, path: &[&str], data: &[u8]) -> Result<()> {
    let (device, rest) = self.device(path)?;
    device.write(activity, rest, data)
  }
}
