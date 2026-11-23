//! Where devices are found: those that serve themselves, as TREE.md's
//! Serve says, and the browser's bridge.

use crate::tree::{Focus, Result};
use std::ffi::OsStr;
use std::process::Command;

/// What answers a device's commands, as PROTOCOL.md or BROWSER.md give
/// them: a device that serves itself, or the browser's extension
pub trait Handler: Send + Sync {
  fn run(&self, args: &[&OsStr]) -> Result<Vec<u8>>;
  /// Where the device is, so that hunch-host tells devices apart
  fn addr(&self) -> &str;
}

impl Handler for std::sync::Arc<dyn Handler> {
  fn run(&self, args: &[&OsStr]) -> Result<Vec<u8>> {
    self.as_ref().run(args)
  }

  fn addr(&self) -> &str {
    self.as_ref().addr()
  }
}

/// A registry of one device, for a program that serves only it
pub struct One(pub std::sync::Arc<dyn Handler>);

impl Registry for One {
  fn find(
    &self,
    _role: &str,
    _activity: Option<&str>,
  ) -> Option<Box<dyn Handler>> {
    Some(Box::new(self.0.clone()))
  }
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

/// The human's windows, as tmux keeps them: an activity is a tmux window
pub struct Tmux;

impl Focus for Tmux {
  /// The window of the client the human used last
  fn current(&self) -> Option<String> {
    let out = Command::new("tmux")
      .args(["list-clients", "-F", "#{client_activity} #{window_id}"])
      .output()
      .ok()?;
    let clients = String::from_utf8(out.stdout).ok()?;
    clients
      .lines()
      .filter_map(|l| l.split_once(' '))
      .max_by_key(|(at, _)| at.parse::<u64>().unwrap_or(0))
      .map(|(_, window)| window.to_string())
  }
}
