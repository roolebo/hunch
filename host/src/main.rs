use hunch::host::{root, serve};
use hunch::tree::Workspace;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::UnixListener;
use std::process::ExitCode;
use std::sync::Arc;

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

  // The assembly: no devices yet
  let workspace = Workspace::new(Vec::new());
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
