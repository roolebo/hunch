//! hunch-host serves what a host has to hunch; skills/hunch/HOST.md
//! specifies it.

mod error;
#[cfg(all(feature = "wm", target_os = "macos"))]
mod macos;
#[cfg(unix)]
mod service;
#[cfg(feature = "wm")]
mod wm;

use error::{Error, Result};
use std::process::ExitCode;

const USAGE: &str = "\
Usage: hunch-host ROLE [ADDR] COMMAND [ARGUMENTS]
       hunch-host wms
       hunch-host needs
       hunch-host serve

Roles:
  wm ADDR   a window manager, e.g. macos: or tmux:/tmp/tmux-1000/default

hunch-host ROLE --help lists the commands of ROLE.
wms prints the window managers not inside another one.
needs prints what the human has yet to grant the service.
serve runs the service, which every other command asks.";

fn run(args: &[String]) -> Result<()> {
    match args {
        [flag] if flag == "-h" || flag == "--help" => println!("{USAGE}"),
        [command] if command == "wms" => println!("{}", serde_json::json!(wms())),
        [command] if command == "needs" => println!("{}", serde_json::Value::from(needs())),
        #[cfg(unix)]
        [command] if command == "serve" => service::serve()?,
        #[cfg(feature = "wm")]
        [role, rest @ ..] if role == "wm" => wm::run(rest)?,
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

fn wms() -> Vec<&'static str> {
    if cfg!(all(feature = "wm", target_os = "macos")) {
        vec!["macos:"]
    } else {
        vec![]
    }
}

/// Whether the command runs in this process rather than in the service:
/// the service itself, what the service runs, and help, which needs none
fn runs_here(args: &[String]) -> bool {
    let help = |arg: &String| arg == "-h" || arg == "--help";
    std::env::var_os(service::SERVED).is_some()
        || args.first().is_none_or(|arg| arg == "serve" || help(arg))
        || args.get(1).is_some_and(help)
}

fn needs() -> Vec<serde_json::Value> {
    #[allow(unused_mut)]
    let mut needs = Vec::new();
    #[cfg(all(feature = "wm", target_os = "macos"))]
    if !macos::may_record_screen() {
        needs.push(serde_json::json!({"for": "window titles", "grant": "Screen Recording"}));
    }
    needs
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
            eprintln!("hunch-host: {error}");
            ExitCode::from(error.status())
        }
    }
}
