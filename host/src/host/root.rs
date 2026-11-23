//! Where hunch keeps its files on a host, as TREE.md's Access says.

use std::fs::{self, DirBuilder, File, OpenOptions, TryLockError};
use std::io::{self, Write};
use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;

/// $XDG_RUNTIME_DIR/hunch, else $XDG_CACHE_HOME/hunch, else
/// ~/.cache/hunch, as GLib picks a runtime directory
pub fn root() -> PathBuf {
  ["XDG_RUNTIME_DIR", "XDG_CACHE_HOME"]
    .into_iter()
    .filter_map(std::env::var_os)
    .find(|dir| !dir.is_empty())
    .map(PathBuf::from)
    .unwrap_or_else(|| {
      PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache")
    })
    .join("hunch")
}

pub fn socket() -> PathBuf {
  root().join("sock").join("hunch.sock")
}

pub fn log() -> PathBuf {
  root().join("hunch-host.log")
}

pub fn tmp() -> PathBuf {
  root().join("tmp")
}

/// Makes ROOT and its directories, for the human alone
pub fn make() -> io::Result<()> {
  let mut b = DirBuilder::new();
  b.recursive(true).mode(0o700);
  b.create(root().join("sock"))?;
  b.create(tmp())
}

/// Locks ROOT/hunch-host.pid for as long as the File lives, or fails if
/// another hunch-host holds it
pub fn lock() -> io::Result<File> {
  let path = root().join("hunch-host.pid");
  let mut file = OpenOptions::new()
    .read(true)
    .write(true)
    .create(true)
    .truncate(false)
    .open(&path)?;
  match file.try_lock() {
    Ok(()) => {}
    Err(TryLockError::WouldBlock) => {
      let pid = fs::read_to_string(&path).unwrap_or_default();
      return Err(io::Error::new(
        io::ErrorKind::AddrInUse,
        format!("hunch-host is already running, pid {}", pid.trim()),
      ));
    }
    Err(TryLockError::Error(e)) => return Err(e),
  }
  file.set_len(0)?;
  writeln!(file, "{}", std::process::id())?;
  Ok(file)
}
