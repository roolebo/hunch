//! Where devices are found: those that serve themselves, as TREE.md's
//! Serve says.

use crate::tree::Result;
use std::ffi::OsStr;

/// What answers a device's commands, as PROTOCOL.md gives them: a device
/// that serves itself
pub trait Handler: Send + Sync {
  fn run(&self, args: &[&OsStr]) -> Result<Vec<u8>>;
  /// Where the device is, so that hunch-host tells devices apart
  fn addr(&self) -> &str;
}

/// The exit status for unsaved changes, as PROTOCOL.md says
pub const UNSAVED: u8 = 3;

pub trait Registry: Send + Sync {
  /// The device with ROLE in ACTIVITY, or with None in any activity
  fn find(
    &self,
    role: &str,
    activity: Option<&str>,
  ) -> Option<Box<dyn Handler>>;
}
