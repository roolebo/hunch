//! Whether an action runs, as TREE.md's Control says.

use crate::tree::Result;

/// Where an action will wait for the human; every action runs for now
pub fn allow(_activity: &str, _path: &[&str]) -> Result<()> {
  Ok(())
}
