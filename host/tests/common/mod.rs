use hunch::client::Client;
use hunch::host::Connection;
use hunch::tree::Tree;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::thread;

/// A client of TREE in ACTIVITY's workspace, with hunch-host's server on the
/// other end of a socket
pub fn attach(tree: Arc<dyn Tree>, activity: &str) -> Client {
  let (ours, theirs) = UnixStream::pair().unwrap();
  thread::spawn(move || {
    let mut reader = theirs.try_clone().unwrap();
    let mut writer = theirs;
    Connection::new(tree.as_ref(), 0)
      .serve(&mut reader, &mut writer)
      .unwrap();
  });
  Client::attach(ours, activity).unwrap()
}
