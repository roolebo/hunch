//! hunch-seat serves the seat of a host, with the window managers and the
//! compute dictation uses; README.md says how.

mod error;
#[cfg(unix)]
mod service;
#[cfg(feature = "wm")]
mod wm;

use error::{Error, Result};
use std::process::ExitCode;

const USAGE: &str = "\
Usage: hunch-seat ROLE [ADDR] COMMAND [ARGUMENTS]
       hunch-seat roles
       hunch-seat wms
       hunch-seat serve

Roles:
  wm ADDR       a window manager, e.g. tmux:/tmp/tmux-1000/default

hunch-seat ROLE --help lists the commands of ROLE.
roles prints the roles this hunch-seat is built with.
wms prints the window managers not inside another one.
serve runs the service, which every other command asks.";

fn run(args: &[String]) -> Result<()> {
  match args {
    [flag] if flag == "-h" || flag == "--help" => println!("{USAGE}"),
    [command] if command == "roles" => {
      println!("{}", serde_json::json!(roles()))
    }
    [command] if command == "wms" => {
      println!("{}", serde_json::json!(wms()))
    }
    #[cfg(unix)]
    [command] if command == "serve" => service::serve(here, start)?,
    #[cfg(feature = "wm")]
    [role, rest @ ..] if role == "wm" => {
      wm::run(rest, &mut std::io::stdin(), &mut std::io::stdout())?
    }
    [] => return Err(Error::Usage("missing ROLE".into(), USAGE)),
    [role, ..] => {
      return Err(Error::Usage(
        format!("unknown role or command, or not built in: {role}"),
        USAGE,
      ));
    }
  }
  Ok(())
}

/// The commands the service answers in its own process
#[cfg(unix)]
fn here(args: &[String]) -> Option<service::Here> {
  match args.first().map(String::as_str) {
    // In the service, which knows the wms that serve it
    Some("wms") if args.len() == 1 => Some(|_, _, out, _| {
      use std::io::Write;
      writeln!(out, "{}", serde_json::json!(wms()))
        .map_err(|e| error::failed(format!("can't answer: {e}")))
    }),
    _ => None,
  }
}

/// What the service starts before it takes clients
#[cfg(unix)]
fn start() {}

fn roles() -> Vec<&'static str> {
  [cfg!(feature = "wm").then_some("wm")]
    .into_iter()
    .flatten()
    .collect()
}

fn wms() -> Vec<&'static str> {
  Vec::new()
}

/// Whether the command runs in this process rather than in the service:
/// the service itself, what the service runs, and help, which needs none
fn runs_here(args: &[String]) -> bool {
  let help = |arg: &String| arg == "-h" || arg == "--help";
  std::env::var_os(service::SERVED).is_some()
    || args.first().is_none_or(|arg| arg == "serve" || help(arg))
    || args.get(1).is_some_and(help)
}

fn main() -> ExitCode {
  let args: Vec<String> = std::env::args().skip(1).collect();
  let status = if runs_here(&args) {
    run(&args).map(|()| 0)
  } else {
    service::ask(&args)
  };
  match status {
    Ok(code) => ExitCode::from(code),
    Err(error) => {
      eprintln!("hunch-seat: {error}");
      ExitCode::from(error.status())
    }
  }
}
