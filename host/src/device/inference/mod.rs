//! inference/, as INFERENCE.md says: the ENDPOINT the workspace's
//! namespace mounts there. local: is a tree hunch-seat serves; a server is
//! one of OpenAI's API, which Server serves here.

mod keys;
mod openai;
mod server;

pub use keys::OsKeys;

use crate::tree::{Kind, Result, Tree};
use server::Server;
use std::path::PathBuf;
use std::sync::Arc;

pub const LOCAL: &str = "local:";
/// Where the namespace mounts it
const MOUNT_POINT: &str = "/inference";

/// Where this host keeps the keys it logs in to servers with
pub trait Keys: Send + Sync {
  fn lookup(&self, endpoint: &str) -> Option<String>;
  fn store(&self, endpoint: &str, key: &str)
  -> std::result::Result<(), String>;
}

pub struct Inference {
  /// local:, as hunch-seat serves it
  local: Box<dyn Tree>,
  keys: Arc<dyn Keys>,
  /// The namespace file, as hunch's README says
  namespace: PathBuf,
}

impl Inference {
  pub fn new(
    local: Box<dyn Tree>,
    keys: Arc<dyn Keys>,
    namespace: PathBuf,
  ) -> Self {
    Inference {
      local,
      keys,
      namespace,
    }
  }

  /// The ENDPOINT the namespace mounts on /inference: its last mount
  /// there, else local:. Read each time, since the human edits it while
  /// hunch-host runs
  fn endpoint(&self) -> String {
    let text = std::fs::read_to_string(&self.namespace).unwrap_or_default();
    let mut endpoint = LOCAL.to_string();
    for line in text.lines().map(str::trim) {
      match line.split_whitespace().collect::<Vec<_>>().as_slice() {
        [] => {}
        [comment, ..] if comment.starts_with('#') => {}
        ["mount", from, MOUNT_POINT] => endpoint = from.to_string(),
        _ => eprintln!(
          "hunch-host: {} has a line it doesn't take: {line}",
          self.namespace.display()
        ),
      }
    }
    endpoint
  }

  /// The tree mounted on inference/ now
  fn mounted<T>(&self, f: impl FnOnce(&dyn Tree) -> T) -> T {
    match self.endpoint().as_str() {
      LOCAL => f(self.local.as_ref()),
      url => f(&Server {
        url: url.into(),
        keys: self.keys.clone(),
      }),
    }
  }
}

impl Tree for Inference {
  fn kind(&self, activity: &str, path: &[&str]) -> Result<Kind> {
    self.mounted(|t| t.kind(activity, path))
  }

  fn list(&self, activity: &str, path: &[&str]) -> Result<Vec<(String, Kind)>> {
    self.mounted(|t| t.list(activity, path))
  }

  fn read(
    &self,
    activity: &str,
    path: &[&str],
    input: Option<&[u8]>,
  ) -> Result<Vec<u8>> {
    self.mounted(|t| t.read(activity, path, input))
  }

  fn write(&self, activity: &str, path: &[&str], data: &[u8]) -> Result<()> {
    self.mounted(|t| t.write(activity, path, data))
  }
}
