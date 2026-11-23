//! hunch-host, the server of the tree.

pub mod control;
pub mod root;
mod server;

pub use server::{Connection, MAX_MSIZE};

use crate::tree::Tree;
use std::os::unix::net::UnixListener;
use std::sync::Arc;
use std::thread;

/// Serves TREE to every client of LISTENER, each on its own thread
pub fn serve(listener: UnixListener, tree: Arc<dyn Tree>, uid: u32) {
  for stream in listener.incoming() {
    let stream = match stream {
      Ok(s) => s,
      Err(e) => {
        eprintln!("hunch-host: can't accept a client: {e}");
        continue;
      }
    };
    let tree = tree.clone();
    thread::spawn(move || {
      let mut reader = match stream.try_clone() {
        Ok(r) => r,
        Err(e) => return eprintln!("hunch-host: {e}"),
      };
      let mut writer = stream;
      if let Err(e) =
        Connection::new(tree.as_ref(), uid).serve(&mut reader, &mut writer)
      {
        eprintln!("hunch-host: a client: {e}");
      }
    });
  }
}
