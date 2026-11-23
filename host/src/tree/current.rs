//! A device that belongs to no activity: it's in the current
//! activity's workspace only, as TREE.md says.

use crate::p9::errno;
use crate::tree::{Error, Kind, Result, Tree};
use std::sync::Arc;

/// Where the human is
pub trait Focus: Send + Sync {
  /// The activity the human is in now
  fn current(&self) -> Option<String>;
}

/// A device composed with the human's focus
pub struct Current<T> {
  device: T,
  focus: Arc<dyn Focus>,
}

impl<T: Tree> Current<T> {
  pub fn new(device: T, focus: Arc<dyn Focus>) -> Self {
    Current { device, focus }
  }

  fn check(&self, activity: &str) -> Result<()> {
    match self.focus.current() {
      Some(current) if current == activity => Ok(()),
      Some(current) => Err(Error::new(
        errno::ENOENT,
        format!("the human is in activity {current}, not {activity}"),
      )),
      None => Err(Error::new(errno::ENOENT, "the human is in no activity")),
    }
  }
}

impl<T: Tree> Tree for Current<T> {
  fn kind(&self, activity: &str, path: &[&str]) -> Result<Kind> {
    self.check(activity)?;
    self.device.kind(activity, path)
  }

  fn list(&self, activity: &str, path: &[&str]) -> Result<Vec<(String, Kind)>> {
    self.check(activity)?;
    self.device.list(activity, path)
  }

  fn read(
    &self,
    activity: &str,
    path: &[&str],
    input: Option<&[u8]>,
  ) -> Result<Vec<u8>> {
    self.check(activity)?;
    self.device.read(activity, path, input)
  }

  fn write(&self, activity: &str, path: &[&str], data: &[u8]) -> Result<()> {
    self.check(activity)?;
    self.device.write(activity, path, data)
  }
}
