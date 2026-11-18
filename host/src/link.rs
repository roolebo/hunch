//! Links, as skills/hunch/HOST.md specifies them: a seat keeps one to each
//! host in its allows, and tells it where the human is and passes the
//! browser's commands on it.

use crate::browser;
use crate::config;
use crate::error::{Error, Result, failed};
use crate::peer::{Peer, Wire};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// How often the seat tells a host where the human is, and checks its
/// allows
const TICK: Duration = Duration::from_secs(1);
/// How long the seat waits before it links again
const RETRY: Duration = Duration::from_secs(5);

/// A seat linked to this host
struct Linked {
    peer: Arc<Peer>,
    /// What the seat said last, and when
    said: Mutex<(Value, Instant)>,
}

static LINKED: Mutex<Vec<Arc<Linked>>> = Mutex::new(Vec::new());
/// The hosts this seat keeps a link to
static LINKING: Mutex<Option<HashSet<String>>> = Mutex::new(None);

fn linked() -> std::sync::MutexGuard<'static, Vec<Arc<Linked>>> {
    LINKED.lock().unwrap_or_else(|e| e.into_inner())
}

/// This host's name, without its domain
fn hostname() -> String {
    let mut name = [0u8; 256];
    // SAFETY: gethostname writes at most len bytes
    unsafe { libc::gethostname(name.as_mut_ptr().cast(), name.len()) };
    let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
    let name = String::from_utf8_lossy(&name[..end]);
    name.split('.').next().unwrap_or_default().to_string()
}

/// IDLE of this host's seat, where it can tell
pub fn idle() -> Option<f64> {
    #[cfg(all(feature = "seat", target_os = "macos"))]
    return Some(crate::macos::idle());
    #[cfg(not(all(feature = "seat", target_os = "macos")))]
    None
}

fn locked() -> Option<bool> {
    #[cfg(all(feature = "seat", target_os = "macos"))]
    return Some(crate::macos::locked());
    #[cfg(not(all(feature = "seat", target_os = "macos")))]
    None
}

/// This host's seat, as seats lists it
fn own() -> Value {
    json!({"host": hostname(), "locked": locked(), "idle": idle()})
}

/// A linked seat as it is now: its IDLE grew since it said it
fn now(linked: &Linked) -> Value {
    let (said, at) = &*linked.said.lock().unwrap_or_else(|e| e.into_inner());
    let mut seat = said["seat"].clone();
    if let Some(idle) = seat["idle"].as_f64() {
        seat["idle"] = json!(idle + at.elapsed().as_secs_f64());
    }
    seat
}

/// The linked seats whose browsers serve this host, each with its IDLE,
/// INFINITY where it can't tell
pub fn serving() -> Vec<(f64, Arc<Peer>)> {
    linked()
        .iter()
        .filter(|l| {
            let said = &l.said.lock().unwrap_or_else(|e| e.into_inner()).0;
            said["browsers"].as_array().is_some_and(|b| !b.is_empty())
        })
        .map(|l| {
            let idle = now(l)["idle"].as_f64().unwrap_or(f64::INFINITY);
            (idle, Arc::clone(&l.peer))
        })
        .collect()
}

/// Takes a seat's link until it goes, on the ssh's pipes
fn take(input: &mut File, out: &mut File) -> Result<()> {
    let mut lines = BufReader::new(input).lines();
    let read = |line: Option<std::io::Result<String>>| -> Result<Option<Value>> {
        match line {
            None => Ok(None),
            Some(line) => {
                let line = line.map_err(|e| failed(format!("can't read the link: {e}")))?;
                serde_json::from_str(&line)
                    .map(Some)
                    .map_err(|e| failed(format!("the link sent {line:?}, not JSON: {e}")))
            }
        }
    };
    let said = read(lines.next())?.ok_or_else(|| failed("the seat went away at once"))?;
    let name = said["seat"]["host"]
        .as_str()
        .ok_or_else(|| failed(format!("the seat said {said}, not where it is")))?;
    let seat = Arc::new(Linked {
        peer: Peer::new(name, Wire::Lines, out)?,
        said: Mutex::new((said.clone(), Instant::now())),
    });
    linked().push(Arc::clone(&seat));
    let taken = loop {
        match read(lines.next()) {
            Ok(Some(message)) if message.get("seat").is_some() => {
                *seat.said.lock().unwrap_or_else(|e| e.into_inner()) = (message, Instant::now());
            }
            Ok(Some(message)) => seat.peer.answer(&message),
            Ok(None) => break Ok(()),
            Err(e) => break Err(e),
        }
    };
    linked().retain(|l| !Arc::ptr_eq(l, &seat));
    seat.peer.close();
    taken
}

/// Keeps a link to each host in the seat's allows, for as long as it's
/// there
pub fn start() {
    thread::spawn(|| {
        loop {
            for host in config::allowed() {
                let mut linking = LINKING.lock().unwrap_or_else(|e| e.into_inner());
                if linking.get_or_insert_default().insert(host.clone()) {
                    thread::spawn(move || keep(host));
                }
            }
            thread::sleep(TICK);
        }
    });
}

fn allowed(host: &str) -> bool {
    config::allowed().iter().any(|h| h == host)
}

fn keep(host: String) {
    let mut failed_last = None;
    while allowed(&host) {
        let linked = link(&host);
        if let Err(e) = &linked {
            // Once, rather than each time it retries
            let message = e.to_string();
            if failed_last.as_ref() != Some(&message) {
                eprintln!("hunch-host: {message}");
            }
            failed_last = Some(message);
        } else {
            failed_last = None;
        }
        thread::sleep(RETRY);
    }
    if let Some(linking) = LINKING.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        linking.remove(&host);
    }
}

/// What the seat says to host each TICK
fn presence(host: &str) -> Value {
    let browsers = if browser::serves() == host {
        browser::names()
    } else {
        vec![]
    };
    json!({"seat": own(), "browsers": browsers})
}

/// Sends a message on the link, unless the seat closed it
fn say(out: &Mutex<Option<ChildStdin>>, message: &Value) -> std::io::Result<()> {
    match out.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        Some(out) => writeln!(out, "{message}"),
        None => Err(std::io::ErrorKind::BrokenPipe.into()),
    }
}

/// Links to host until the link breaks or host leaves the allows
fn link(host: &str) -> Result<()> {
    let mut child = Command::new("ssh")
        // The service has no one to ask for a password or a host key
        .args(["-o", "BatchMode=yes", host, "hunch-host", "link"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| failed(format!("can't run ssh to link {host}: {e}")))?;
    // None once the seat closes it, which ends the link on host
    let out = Arc::new(Mutex::new(child.stdin.take()));
    let input = child.stdout.take().expect("piped");
    let mut stderr = child.stderr.take().expect("piped");
    let errors = thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });
    let presenting = {
        let out = Arc::clone(&out);
        let host = host.to_string();
        thread::spawn(move || {
            while allowed(&host) {
                if say(&out, &presence(&host)).is_err() {
                    return;
                }
                thread::sleep(TICK);
            }
            out.lock().unwrap_or_else(|e| e.into_inner()).take();
        })
    };
    for line in BufReader::new(input).lines() {
        let Ok(line) = line else { break };
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            eprintln!("hunch-host: {host} sent {line:?} on its link, not JSON");
            continue;
        };
        let out = Arc::clone(&out);
        let host = host.to_string();
        // A command can take long, and the next one shouldn't wait for it
        thread::spawn(move || {
            let command = message["command"].as_str().unwrap_or_default();
            let args: Vec<&str> = message["args"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            let reply = match browser::answer(&host, command, &args) {
                Ok(result) => json!({"id": message["id"], "result": result}),
                Err(error) => json!({"id": message["id"], "error": error}),
            };
            let _ = say(&out, &reply);
        });
    }
    let status = child.wait();
    let _ = presenting.join();
    let errors = errors.join().unwrap_or_default();
    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(failed(format!(
            "the link to {host} broke ({status}): {}",
            errors.trim()
        ))),
        Err(e) => Err(failed(format!("can't wait for the link to {host}: {e}"))),
    }
}

/// What the seat's ssh runs on this host: takes the link in the service
pub fn run_link(args: &[String], input: &mut File, out: &mut File, _: &AtomicBool) -> Result<()> {
    if !args.is_empty() {
        return Err(Error::Usage("link takes no arguments".into(), crate::USAGE));
    }
    take(input, out)
}

pub fn run_seats(args: &[String], _: &mut File, out: &mut File, _: &AtomicBool) -> Result<()> {
    if !args.is_empty() {
        return Err(Error::Usage(
            "seats takes no arguments".into(),
            crate::USAGE,
        ));
    }
    let mut seats: Vec<Value> = linked().iter().map(|l| now(l)).collect();
    if idle().is_some() {
        seats.insert(0, own());
    }
    writeln!(out, "{}", Value::from(seats)).map_err(|e| failed(format!("can't answer: {e}")))
}
