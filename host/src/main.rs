//! hunch-host serves what a host has to hunch; skills/hunch/HOST.md
//! specifies it.

#[cfg(all(unix, feature = "browser"))]
mod browser;
#[cfg(feature = "compute")]
mod compute;
#[cfg(all(unix, feature = "browser"))]
mod config;
mod error;
#[cfg(all(unix, feature = "browser"))]
mod link;
#[cfg(all(feature = "wm", target_os = "macos"))]
mod macos;
#[cfg(all(feature = "seat", target_os = "macos"))]
mod seat;
#[cfg(unix)]
mod service;
#[cfg(any(feature = "compute", feature = "seat"))]
mod wav;
#[cfg(feature = "wm")]
mod wm;

use error::{Error, Result};
use std::process::ExitCode;

const USAGE: &str = "\
Usage: hunch-host ROLE [ADDR] COMMAND [ARGUMENTS]
       hunch-host wms
       hunch-host seats
       hunch-host link
       hunch-host needs [--ask] [--all]
       hunch-host serve

Roles:
  wm ADDR       a window manager, e.g. macos: or tmux:/tmp/tmux-1000/default
  compute ADDR  runs models, e.g. local:
  seat          where the human sits: dictation
  browser       the browser the human uses at the seat

hunch-host ROLE --help lists the commands of ROLE.
wms prints the window managers not inside another one.
seats prints the seats linked to this host, and its own.
link takes a seat's link, on stdin and stdout; the seat runs it by ssh.
needs prints what the human has yet to grant the service; --ask has the
OS ask the human for it, where the OS can; --all lists every grant, each
with whether it's granted.
serve runs the service, which every other command asks.";

fn run(args: &[String]) -> Result<()> {
    match args {
        [flag] if flag == "-h" || flag == "--help" => println!("{USAGE}"),
        [command] if command == "wms" => println!("{}", serde_json::json!(wms())),
        [command, flags @ ..] if command == "needs" => needs(flags)?,
        #[cfg(unix)]
        [command] if command == "serve" => service::serve(here, start)?,
        #[cfg(all(feature = "seat", target_os = "macos"))]
        [role, flag] if role == "seat" && (flag == "-h" || flag == "--help") => {
            println!("{}", seat::USAGE)
        }
        #[cfg(all(feature = "seat", target_os = "macos"))]
        [role, ..] if role == "seat" => {
            return Err(Error::Failed(
                "the seat lives in the service: run it with hunch-host serve".into(),
            ));
        }
        #[cfg(all(unix, feature = "browser"))]
        [role, flag] if role == "browser" && (flag == "-h" || flag == "--help") => {
            println!("{}", browser::USAGE)
        }
        #[cfg(feature = "wm")]
        [role, rest @ ..] if role == "wm" => wm::run(rest)?,
        #[cfg(feature = "compute")]
        [role, rest @ ..] if role == "compute" => {
            let ran = compute::run(rest, &mut std::io::stdin(), &mut std::io::stdout());
            compute::unload();
            ran?
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
        #[cfg(all(feature = "seat", target_os = "macos"))]
        Some("seat") if args.get(1).is_none_or(|a| a != "-h" && a != "--help") => Some(seat::run),
        // In the service, which holds the extensions' connections
        #[cfg(feature = "browser")]
        Some("browser") => Some(browser::run),
        // In the service, which holds the links
        #[cfg(feature = "browser")]
        Some("link") => Some(link::run_link),
        #[cfg(feature = "browser")]
        Some("seats") => Some(link::run_seats),
        // In the service, which keeps the model loaded between runs
        #[cfg(feature = "compute")]
        Some("compute") => Some(|args, input, out, _| compute::run(args, input, out)),
        _ => None,
    }
}

/// What the service starts before it takes clients
#[cfg(unix)]
fn start() {
    #[cfg(all(feature = "seat", target_os = "macos"))]
    seat::start();
    #[cfg(feature = "browser")]
    link::start();
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

/// A grant the service uses
struct Grant {
    what: &'static str,
    grant: &'static str,
    granted: bool,
}

/// The grants this build uses on this host
#[allow(unused_variables)]
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
        // Accessibility lets the dictation key's tap listen too, so the
        // human never needs to grant Input Monitoring
        grants.push(Grant {
            what: if cfg!(feature = "seat") {
                "sending text, picking windows, the dictation key"
            } else {
                "sending text, picking windows"
            },
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
    println!("{}", serde_json::Value::from(listed));
    Ok(())
}

fn main() -> ExitCode {
    #[cfg_attr(not(all(unix, feature = "browser")), allow(unused_mut))]
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    // The browser runs it by this name for its extension, with arguments
    // of its own
    #[cfg(all(unix, feature = "browser"))]
    if std::env::args_os()
        .next()
        .is_some_and(|exe| std::path::Path::new(&exe).file_name() == Some(browser::NATIVE.as_ref()))
    {
        args = vec!["browser".into(), "connect".into()];
    }
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
