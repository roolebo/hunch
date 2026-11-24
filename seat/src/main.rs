//! hunch-seat serves the seat of a host, with the window managers
//! dictation types into and the speech models this host runs; README.md
//! says how.

mod error;
#[cfg(feature = "inference")]
mod inference;
#[cfg(all(feature = "wm", target_os = "macos"))]
mod macos;
#[cfg(all(feature = "seat", any(target_os = "macos", target_os = "linux")))]
mod seat;
#[cfg(unix)]
mod service;
#[cfg(feature = "inference")]
mod wav;
#[cfg(feature = "wm")]
mod wm;

use error::{Error, Result};
use std::process::ExitCode;

const USAGE: &str = "\
Usage: hunch-seat ROLE [ADDR] COMMAND [ARGUMENTS]
       hunch-seat roles
       hunch-seat wms
       hunch-seat needs [--ask] [--all]
       hunch-seat serve

Roles:
  wm ADDR       a window manager, e.g. macos: or tmux:/tmp/tmux-1000/default
  seat          where the human sits: dictation

hunch-seat ROLE --help lists the commands of ROLE.
roles prints the roles this hunch-seat is built with.
wms prints the window managers not inside another one.
needs prints what the human has yet to grant the service; --ask has the
OS ask the human for it, where the OS can; --all lists every grant, each
with whether it's granted.
serve runs the service, which every other command asks; it serves local:,
the speech models this host runs, as INFERENCE.md in hunch says.";

fn run(args: &[String]) -> Result<()> {
  match args {
    [flag] if flag == "-h" || flag == "--help" => println!("{USAGE}"),
    [command] if command == "roles" => {
      println!("{}", serde_json::json!(roles()))
    }
    [command] if command == "wms" => {
      println!("{}", serde_json::json!(wms()))
    }
    [command, flags @ ..] if command == "needs" => needs(flags)?,
    #[cfg(unix)]
    [command] if command == "serve" => service::serve(here, start)?,
    #[cfg(all(
      feature = "seat",
      any(target_os = "macos", target_os = "linux")
    ))]
    [role, flag] if role == "seat" && (flag == "-h" || flag == "--help") => {
      println!("{}", seat::USAGE)
    }
    #[cfg(all(
      feature = "seat",
      any(target_os = "macos", target_os = "linux")
    ))]
    [role, ..] if role == "seat" => {
      return Err(Error::Failed(
        "the seat lives in the service: run it with hunch-seat serve".into(),
      ));
    }
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
    #[cfg(all(
      feature = "seat",
      any(target_os = "macos", target_os = "linux")
    ))]
    Some("seat") if args.get(1).is_none_or(|a| a != "-h" && a != "--help") => {
      Some(seat::run)
    }
    // In the service, which holds the Shell extension's connection
    #[cfg(all(feature = "wm", target_os = "linux"))]
    Some("wm") if args.get(1).is_some_and(|a| a == wm::gnome::ADDR) => {
      Some(wm::run_here)
    }
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
fn start() {
  #[cfg(feature = "inference")]
  inference::serve();
}

fn roles() -> Vec<&'static str> {
  [
    cfg!(feature = "wm").then_some("wm"),
    cfg!(feature = "inference").then_some("inference"),
    cfg!(all(
      feature = "seat",
      any(target_os = "macos", target_os = "linux")
    ))
    .then_some("seat"),
  ]
  .into_iter()
  .flatten()
  .collect()
}

fn wms() -> Vec<&'static str> {
  [
    cfg!(all(feature = "wm", target_os = "macos")).then_some("macos:"),
    #[cfg(all(feature = "wm", target_os = "linux"))]
    wm::gnome::served().then_some(wm::gnome::ADDR),
  ]
  .into_iter()
  .flatten()
  .collect()
}

/// Whether the command runs in this process rather than in the service:
/// the service itself, what the service runs, and help, which needs none
fn runs_here(args: &[String]) -> bool {
  let help = |arg: &String| arg == "-h" || arg == "--help";
  std::env::var_os(service::SERVED).is_some()
    || args.first().is_none_or(|arg| arg == "serve" || help(arg))
    || args.get(1).is_some_and(help)
}

/// A grant the service uses
struct Grant {
  what: &'static str,
  grant: &'static str,
  granted: bool,
}

/// The grants this build uses on this host, which depend on its OS
#[allow(unused_variables, clippy::vec_init_then_push)]
fn grants(ask: bool) -> Vec<Grant> {
  #[allow(unused_mut)]
  let mut grants = Vec::new();
  #[cfg(all(feature = "wm", target_os = "macos"))]
  {
    let screen = macos::may_record_screen();
    if ask && !screen {
      macos::ask_to_record_screen();
    }
    grants.push(Grant {
      what: "window titles",
      grant: "Screen Recording",
      granted: screen,
    });
    let events = macos::may_post_events();
    if ask && !events {
      macos::ask_to_post_events();
    }
    grants.push(Grant {
      what: "sending text, picking windows",
      grant: "Accessibility",
      granted: events,
    });
    // Only the app can ask for the microphone: the request answers
    // through a callback, which the service has no run loop for
    #[cfg(feature = "seat")]
    grants.push(Grant {
      what: "dictation",
      grant: "Microphone",
      granted: macos::may_record_audio(),
    });
  }
  grants
}

fn needs(flags: &[String]) -> Result<()> {
  let ask = flags.iter().any(|f| f == "--ask");
  let all = flags.iter().any(|f| f == "--all");
  if let Some(flag) = flags.iter().find(|f| *f != "--ask" && *f != "--all") {
    return Err(Error::Usage(format!("unknown flag: {flag}"), USAGE));
  }
  println!("{}", listed(ask, all));
  Ok(())
}

/// The grants, as needs prints them: the missing ones, or ALL
fn listed(ask: bool, all: bool) -> serde_json::Value {
  let listed: Vec<serde_json::Value> = grants(ask)
    .into_iter()
    .filter(|g| all || !g.granted)
    .map(|g| {
      let mut need = serde_json::json!({"for": g.what, "grant": g.grant});
      if all {
        need["granted"] = g.granted.into();
      }
      need
    })
    .collect();
  listed.into()
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
