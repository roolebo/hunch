//! hunch-host serves what a host has to hunch; skills/hunch/HOST.md
//! specifies it.

mod error;
#[cfg(feature = "wm")]
mod wm;

use error::{Error, Result};
use std::process::ExitCode;

const USAGE: &str = "\
Usage: hunch-host ROLE [ADDR] COMMAND [ARGUMENTS]

Roles:
  wm ADDR   a window manager, e.g. tmux:/tmp/tmux-1000/default

hunch-host ROLE --help lists the commands of ROLE.";

fn run(args: &[String]) -> Result<()> {
    match args {
        [flag] if flag == "-h" || flag == "--help" => println!("{USAGE}"),
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

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("hunch-host: {error}");
            ExitCode::from(error.status())
        }
    }
}
