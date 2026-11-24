use hunch::client::Client;
use hunch::host::{self, root};
use hunch::p9::errno;
use hunch::registry::UNSAVED;
use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};
use std::thread::sleep;
use std::time::Duration;

const USAGE: &str = "\
Usage: hunch-9p [--activity ACTIVITY] COMMAND PATH

Commands:
  read PATH     Print the file
  write PATH    Write stdin to the file
  rdwr PATH     Write stdin to the file, then print its answer
  ls PATH       List the directory, a directory's name with a / after it
  serve KIND    Serve the device that runs this, of KIND, e.g. editor, to
                the workspace, until stdin ends: it gets the device's
                commands on stdout and answers on stdin, as TREE.md's
                Serve says

Reaches ACTIVITY's workspace in hunch-host: in tmux, by default, the
window's; else the host's. Starts hunch-host if it isn't running. Exits
with 3 when the file is busy, e.g. the human has unsaved changes in the
editor.";

/// START_TRIES × START_WAIT: how long a hunch-host this starts may take to
/// listen
const START_TRIES: u32 = 50;
const START_WAIT: Duration = Duration::from_millis(100);

/// Starts hunch-host on its own, so that it outlives the shell that
/// started it
fn start() -> io::Result<()> {
  let program = host::sibling("hunch-host");
  root::make()?;
  let log = OpenOptions::new()
    .create(true)
    .append(true)
    .open(root::log())?;
  Command::new(&program)
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(log)
    .process_group(0)
    .spawn()
    .map_err(|e| {
      io::Error::new(
        e.kind(),
        format!("can't start {}: {e}", program.display()),
      )
    })?;
  Ok(())
}

/// The connection to hunch-host's SOCKET, after starting it if needed
fn connect(socket: &Path) -> io::Result<UnixStream> {
  match UnixStream::connect(socket) {
    Ok(s) => return Ok(s),
    Err(e)
      if matches!(
        e.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
      ) => {}
    Err(e) => return Err(e),
  }
  start()?;
  for _ in 0..START_TRIES {
    sleep(START_WAIT);
    if let Ok(s) = UnixStream::connect(socket) {
      return Ok(s);
    }
  }
  Err(io::Error::other(format!(
    "hunch-host doesn't listen at {}; see {}",
    socket.display(),
    root::log().display()
  )))
}

/// The tmux window this runs in, as an activity
fn window() -> Option<String> {
  let pane = std::env::var("TMUX_PANE").ok()?;
  let out = Command::new("tmux")
    .args(["display", "-p", "-t", &pane, "#{window_id}"])
    .output()
    .ok()?;
  let window = String::from_utf8(out.stdout).ok()?.trim().to_string();
  (out.status.success() && !window.is_empty()).then_some(window)
}

/// Hands stdin and stdout to hunch-host as the device of KIND, until
/// either end closes
fn serve(activity: &str, kind: &str) -> io::Result<()> {
  let stream = connect(&root::srv())?;
  let header = serde_json::json!({"activity": activity, "kind": kind});
  writeln!(&stream, "{header}")?;
  let mut to_host = stream.try_clone()?;
  let sending = std::thread::spawn(move || {
    let copied = io::copy(&mut io::stdin(), &mut to_host);
    let _ = to_host.shutdown(std::net::Shutdown::Write);
    copied
  });
  io::copy(&mut &stream, &mut io::stdout())?;
  drop(sending);
  Ok(())
}

fn stdin() -> io::Result<Vec<u8>> {
  let mut data = Vec::new();
  io::stdin().read_to_end(&mut data)?;
  Ok(data)
}

fn main() -> ExitCode {
  let args: Vec<String> = std::env::args().skip(1).collect();
  let (activity, command, path) = match args.as_slice() {
    [flag] if flag == "-h" || flag == "--help" => {
      println!("{USAGE}");
      return ExitCode::SUCCESS;
    }
    [flag, activity, command, path] if flag == "--activity" => {
      (activity.clone(), command, path)
    }
    [command, path] => (window().unwrap_or_default(), command, path),
    _ => {
      eprintln!("{USAGE}");
      return ExitCode::FAILURE;
    }
  };
  let fail = |what: String| {
    eprintln!("hunch-9p: {what}");
    ExitCode::FAILURE
  };
  if command == "serve" {
    return match serve(&activity, path) {
      Ok(()) => ExitCode::SUCCESS,
      Err(e) => fail(format!("{path}: {e}")),
    };
  }
  let stream = match connect(&root::socket()) {
    Ok(s) => s,
    Err(e) => return fail(e.to_string()),
  };
  let mut client = match Client::attach(stream, &activity) {
    Ok(c) => c,
    Err(e) => {
      return fail(format!("{}: {}", errno::describe(e.errno), e.message));
    }
  };
  let result = match command.as_str() {
    "read" => client.read(path),
    "write" => match stdin() {
      Ok(data) => client.write(path, &data).map(|()| Vec::new()),
      Err(e) => return fail(format!("can't read stdin: {e}")),
    },
    "rdwr" => match stdin() {
      Ok(data) => client.rdwr(path, &data),
      Err(e) => return fail(format!("can't read stdin: {e}")),
    },
    "ls" => client.ls(path).map(|names| {
      names
        .iter()
        .map(|n| format!("{n}\n"))
        .collect::<String>()
        .into_bytes()
    }),
    _ => {
      eprintln!("{USAGE}");
      return ExitCode::FAILURE;
    }
  };
  match result {
    Ok(data) => match io::stdout().write_all(&data) {
      Ok(()) => ExitCode::SUCCESS,
      Err(e) => fail(format!("can't write stdout: {e}")),
    },
    Err(e) => {
      let words = if e.message.is_empty() {
        String::new()
      } else {
        format!(": {}", e.message)
      };
      eprintln!("hunch-9p: {path}: {}{words}", errno::describe(e.errno));
      if e.errno == errno::EBUSY {
        ExitCode::from(UNSAVED)
      } else {
        ExitCode::FAILURE
      }
    }
  }
}
