use hunch::device;
use hunch::host::{root, serve};
use hunch::native::{self, Extension};
use hunch::registry::{Handler, One};
use hunch::tree::Tree;
use serde_json::json;
use std::fs;
use std::io;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::thread;

const USAGE: &str = "\
Usage: hunch-browser-native MANIFEST [EXTENSION]
       hunch-browser-native --install

Firefox runs it for hunch's extension, as the native messaging host
hunch: it serves the browser at ROOT/sock/browser.sock, where hunch-host
finds it, and passes the commands to the extension. When the human
switches browsers, the one they used last takes the socket.

--install writes the manifest that lets Firefox run it for this user.";

/// Native messaging's name for it, which the extension connects to
const HOST: &str = "hunch";
const EXTENSION: &str = "firefox-hunch@hunch";

fn home() -> PathBuf {
  PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

/// Where Firefox looks for a user's native messaging hosts
fn manifests() -> PathBuf {
  if cfg!(target_os = "macos") {
    home().join("Library/Application Support/Mozilla/NativeMessagingHosts")
  } else {
    home().join(".mozilla/native-messaging-hosts")
  }
}

fn install() -> io::Result<PathBuf> {
  let exe = std::env::current_exe()?.canonicalize()?;
  let manifest = manifests().join(format!("{HOST}.json"));
  let json = json!({
    "name": HOST,
    "description": "hunch-browser-native, which serves the browser to hunch",
    "path": exe,
    "type": "stdio",
    "allowed_extensions": [EXTENSION],
  });
  fs::create_dir_all(manifests())?;
  fs::write(&manifest, format!("{json:#}\n"))?;
  Ok(manifest)
}

/// Serves TREE at the browser's socket, in place of what was there;
/// the socket's inode, to tell it's still ours
fn take(tree: &Arc<dyn Tree>) -> io::Result<u64> {
  let socket = root::browser();
  match fs::remove_file(&socket) {
    Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
    _ => {}
  }
  let listener = UnixListener::bind(&socket)?;
  fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
  let uid = fs::metadata(root::root())?.uid();
  let tree = tree.clone();
  thread::spawn(move || serve(listener, tree, uid));
  Ok(fs::metadata(&socket)?.ino())
}

fn bridge() -> io::Result<()> {
  root::make()?;
  let extension = Extension::new(Box::new(io::stdout()));
  let handler: Arc<dyn Handler> = extension.clone();
  let tree: Arc<dyn Tree> = Arc::new(device::browser(Arc::new(One(handler))));
  let mut ours = take(&tree)?;
  let mut input = io::stdin().lock();
  while let Some(message) = native::receive(&mut input)? {
    if let Some(name) = message["hello"]["name"].as_str() {
      eprintln!("hunch-browser-native: serving {name}");
    } else if message["focus"] == true {
      ours = take(&tree)?;
    } else if !extension.answer(&message) {
      eprintln!("hunch-browser-native: a message it doesn't know: {message}");
    }
  }
  extension.gone();
  // Another browser's, if it took the socket since
  let socket = root::browser();
  if fs::metadata(&socket).is_ok_and(|m| m.ino() == ours) {
    let _ = fs::remove_file(&socket);
  }
  Ok(())
}

fn main() -> ExitCode {
  let args: Vec<String> = std::env::args().skip(1).collect();
  let done = match args.as_slice() {
    [flag] if flag == "-h" || flag == "--help" => {
      println!("{USAGE}");
      return ExitCode::SUCCESS;
    }
    [flag] if flag == "--install" => install().map(|manifest| {
      println!("Firefox may run it now: {}", manifest.display());
    }),
    // What Firefox passes: the manifest, and on some OSes the extension
    [_] | [_, _] if !args[0].starts_with('-') => bridge(),
    _ => {
      eprintln!("{USAGE}");
      return ExitCode::FAILURE;
    }
  };
  match done {
    Ok(()) => ExitCode::SUCCESS,
    Err(e) => {
      eprintln!("hunch-browser-native: {e}");
      ExitCode::FAILURE
    }
  }
}
