use hunch::device::{self, Inference, OsKeys};
use hunch::host::{root, serve, sibling};
use hunch::registry::{Program, Tmux};
use hunch::srv::Srv;
use hunch::tree::{ByActivity, Current, Remote, Union, Workspace};
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::UnixListener;
use std::process::ExitCode;
use std::sync::Arc;
use std::thread;

const USAGE: &str = "\
Usage: hunch-host

Serves the workspaces of this host and its activities over 9P2000.L, at
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
  let seat = || {
    device::seat(Box::new(Program {
      handler: sibling("hunch-seat").to_string_lossy().into(),
      addr: "seat".into(),
    }))
  };
  // The host's, in every workspace, as its namespace mounts it;
  // hunch-seat serves local:
  let inference = || {
    Inference::new(
      Box::new(Remote::new(
        root::inference(),
        "no local: hunch-seat serves it where it runs models",
      )),
      Arc::new(OsKeys),
      root::config().join("namespace"),
    )
  };
  // hunch-browser-native serves it, while the browser runs
  let browser = || {
    Remote::new(
      root::browser(),
      "no browser: Firefox serves it while hunch's extension runs",
    )
  };
  // The seat's own files, and what's on it: the activity's editor, which
  // serves itself, and the browser
  let host = Workspace::new(vec![
    ("inference", Box::new(inference())),
    (
      "seat",
      Box::new(Union::new(vec![
        Box::new(seat()),
        Box::new(Workspace::new(vec![("browser", Box::new(browser()))])),
      ])),
    ),
  ]);
  let activity = Workspace::new(vec![
    ("inference", Box::new(inference())),
    (
      "seat",
      Box::new(Union::new(vec![
        Box::new(Current::new(seat(), tmux.clone())),
        Box::new(Workspace::new(vec![
          ("editor", Box::new(device::editor(srv, root::tmp()))),
          ("browser", Box::new(Current::new(browser(), tmux))),
        ])),
      ])),
    ),
  ]);
  eprintln!("hunch-host: serving at {}", socket.display());
  serve(
    listener,
    Arc::new(ByActivity::new(Box::new(host), Box::new(activity))),
    uid,
  );
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
