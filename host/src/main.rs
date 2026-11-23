use hunch::device;
use hunch::host::{root, serve};
use hunch::registry::Tmux;
use hunch::srv::Srv;
use hunch::tree::Workspace;
use hunch::tree::{Current, Remote};
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::UnixListener;
use std::process::ExitCode;
use std::sync::Arc;
use std::thread;

const USAGE: &str = "\
Usage: hunch-host

Serves the workspaces of the activities on this host over 9P2000.L, at
ROOT/sock/hunch.sock; skills/hunch/TREE.md in hunch specifies them.
hunch-9p starts it when it isn't running.";

fn run() -> std::io::Result<()> {
  root::make()?;
  let _lock = root::lock()?;
  let socket = root::socket();
  // The lock is ours, so a socket here is an earlier run's
  match fs::remove_file(&socket) {
    Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
    _ => {}
  }
  let listener = UnixListener::bind(&socket)?;
  fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
  let uid = fs::metadata(root::root())?.uid();
  let srv_socket = root::srv();
  match fs::remove_file(&srv_socket) {
    Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
    _ => {}
  }
  let srv = Arc::new(Srv::default());
  let srv_listener = UnixListener::bind(&srv_socket)?;
  fs::set_permissions(&srv_socket, fs::Permissions::from_mode(0o600))?;
  thread::spawn({
    let srv = srv.clone();
    move || srv.serve(srv_listener)
  });

  let tmux = Arc::new(Tmux);
  // What's on the seat: the activity's editor, which serves itself, and
  // the browser, which hunch-browser-native serves while the browser runs
  let seat = Workspace::new(vec![
    ("editor", Box::new(device::editor(srv, root::tmp()))),
    (
      "browser",
      Box::new(Current::new(
        Remote::new(
          root::browser(),
          "no browser: Firefox serves it while hunch's extension runs",
        ),
        tmux,
      )),
    ),
  ]);
  let workspace = Workspace::new(vec![("seat", Box::new(seat))]);
  eprintln!("hunch-host: serving at {}", socket.display());
  serve(listener, Arc::new(workspace), uid);
  Ok(())
}

fn main() -> ExitCode {
  let args: Vec<String> = std::env::args().skip(1).collect();
  match args.as_slice() {
    [] => {}
    [flag] if flag == "-h" || flag == "--help" => {
      println!("{USAGE}");
      return ExitCode::SUCCESS;
    }
    _ => {
      eprintln!("{USAGE}");
      return ExitCode::FAILURE;
    }
  }
  match run() {
    Ok(()) => ExitCode::SUCCESS,
    Err(e) => {
      eprintln!("hunch-host: {e}");
      ExitCode::FAILURE
    }
  }
}
