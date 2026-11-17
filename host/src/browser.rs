//! The browser role; skills/hunch/HOST.md and BROWSER.md specify it. The
//! browser starts hunch-host-native for its extension, as native
//! messaging does; that's a client of the service like any other, so the
//! service reads and writes the extension's messages on the browser's
//! own pipes.

use crate::error::{Error, Result, failed};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const USAGE: &str = "\
Usage: hunch-host browser COMMAND [ARGUMENTS]

Commands:
  context            Where the human is: tab, title, selection
  selection          The selected text in the active tab
  read               The text of the active tab
  dom [SELECTOR]     HTML of the elements SELECTOR matches, or of the page
  status             The browsers connected, as JSON
  install BROWSER    Let BROWSER's extension reach the service: firefox

BROWSER.md specifies the first four. Commands go to the browser the
human used last.";

/// The name the browser runs hunch-host by, as native messaging's manifest
/// gives it
pub const NATIVE: &str = "hunch-host-native";
/// The native messaging host's name, which the extension connects to
const HOST_NAME: &str = "hunch";
const FIREFOX_EXTENSION: &str = "firefox-hunch@hunch";
/// How long a command waits for the extension's reply
const REPLY_WAIT: Duration = Duration::from_secs(30);

type Reply = std::result::Result<Value, String>;

/// An extension connected through native messaging
struct Browser {
    name: String,
    out: Mutex<File>,
    waiting: Mutex<HashMap<u64, Sender<Reply>>>,
}

/// The connected browsers, the one the human used last at the end
static BROWSERS: Mutex<Vec<Arc<Browser>>> = Mutex::new(Vec::new());
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn browsers() -> std::sync::MutexGuard<'static, Vec<Arc<Browser>>> {
    BROWSERS.lock().unwrap_or_else(|e| e.into_inner())
}

/// A message as native messaging frames it: its length in native byte
/// order, then JSON; None at the end of the stream
fn receive(input: &mut File) -> io::Result<Option<Value>> {
    let mut length = [0; 4];
    match input.read_exact(&mut length) {
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        read => read?,
    }
    let mut message = vec![0; u32::from_ne_bytes(length) as usize];
    input.read_exact(&mut message)?;
    serde_json::from_slice(&message)
        .map(Some)
        .map_err(io::Error::other)
}

fn send(out: &mut File, message: &Value) -> io::Result<()> {
    let message = message.to_string();
    let length = u32::try_from(message.len()).map_err(io::Error::other)?;
    out.write_all(&length.to_ne_bytes())?;
    out.write_all(message.as_bytes())?;
    out.flush()
}

/// Serves one extension until its browser closes the connection. It says
/// hello first, with the browser's name; then come replies, each with the
/// id of its command, and focus when the human switches to this browser.
fn connect(input: &mut File, out: &mut File) -> Result<()> {
    let hello = receive(input)
        .map_err(|e| failed(format!("can't read the extension's hello: {e}")))?
        .ok_or_else(|| failed("the extension went away before its hello"))?;
    let name = hello["hello"]["name"]
        .as_str()
        .ok_or_else(|| failed(format!("the extension said {hello}, not hello")))?;
    let out = out
        .try_clone()
        .map_err(|e| failed(format!("can't keep the extension's output: {e}")))?;
    let browser = Arc::new(Browser {
        name: name.into(),
        out: Mutex::new(out),
        waiting: Mutex::new(HashMap::new()),
    });
    browsers().push(Arc::clone(&browser));
    let received = loop {
        let message = match receive(input) {
            Ok(Some(message)) => message,
            Ok(None) => break Ok(()),
            Err(e) => break Err(failed(format!("can't read {name}'s extension: {e}"))),
        };
        if message["focus"] == true {
            let mut all = browsers();
            all.retain(|b| !Arc::ptr_eq(b, &browser));
            all.push(Arc::clone(&browser));
        } else if let Some(id) = message["id"].as_u64() {
            let reply = match message.get("error").and_then(Value::as_str) {
                Some(error) => Err(error.to_string()),
                None => Ok(message["result"].clone()),
            };
            let waiting = browser
                .waiting
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&id);
            if let Some(waiting) = waiting {
                let _ = waiting.send(reply);
            }
        }
    };
    browsers().retain(|b| !Arc::ptr_eq(b, &browser));
    // Dropping the senders tells each waiting command the browser is gone
    browser
        .waiting
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
    received
}

/// Runs a command of BROWSER.md in the browser the human used last
fn ask(command: &str, args: &[&str]) -> Result<Value> {
    let browser = browsers().last().cloned().ok_or_else(|| {
        failed("no browser is connected: install its extension, see hunch-host browser --help")
    })?;
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let (reply, replied) = mpsc::channel();
    browser
        .waiting
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id, reply);
    let request = json!({"id": id, "command": command, "args": args});
    let sent = send(
        &mut browser.out.lock().unwrap_or_else(|e| e.into_inner()),
        &request,
    );
    if let Err(e) = sent {
        browser
            .waiting
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
        return Err(failed(format!("can't reach {}: {e}", browser.name)));
    }
    let name = &browser.name;
    match replied.recv_timeout(REPLY_WAIT) {
        Ok(reply) => reply.map_err(|e| failed(format!("{name}: {e}"))),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(failed(format!("{name} went away"))),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            browser
                .waiting
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&id);
            Err(failed(format!(
                "{name} didn't answer in {} s",
                REPLY_WAIT.as_secs()
            )))
        }
    }
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

/// Where Firefox looks for the manifests of native messaging hosts
fn firefox_manifests() -> PathBuf {
    if cfg!(target_os = "macos") {
        home().join("Library/Application Support/Mozilla/NativeMessagingHosts")
    } else {
        home().join(".mozilla/native-messaging-hosts")
    }
}

/// Lets Firefox start hunch-host for its extension: a link named NATIVE to
/// this binary, so it knows to connect, and the manifest naming it
fn install_firefox() -> Result<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local/share"))
        .join("hunch");
    let exe = std::env::current_exe().map_err(|e| failed(format!("can't find hunch-host: {e}")))?;
    let link = data.join(NATIVE);
    let manifests = firefox_manifests();
    let manifest = manifests.join(format!("{HOST_NAME}.json"));
    let json = json!({
        "name": HOST_NAME,
        "description": "hunch-host, which serves the browser to hunch",
        "path": link,
        "type": "stdio",
        "allowed_extensions": [FIREFOX_EXTENSION],
    });
    let installed = std::fs::create_dir_all(&data)
        .and_then(|()| match std::fs::remove_file(&link) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => std::os::unix::fs::symlink(&exe, &link),
        })
        .and_then(|()| std::fs::create_dir_all(&manifests))
        .and_then(|()| std::fs::write(&manifest, format!("{json:#}\n")));
    installed.map_err(|e| failed(format!("can't install for Firefox: {e}")))?;
    Ok(manifest)
}

/// Prints a result as BROWSER.md says: text as it is, other JSON as JSON,
/// nothing for none
fn print(out: &mut File, result: &Value) -> io::Result<()> {
    match result {
        Value::Null => Ok(()),
        Value::String(text) => writeln!(out, "{text}"),
        other => writeln!(out, "{other}"),
    }
}

/// Answers a browser command in the service, on the client's stdin and
/// stdout
pub fn run(args: &[String], input: &mut File, out: &mut File, _: &AtomicBool) -> Result<()> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let written = match args.as_slice() {
        ["-h" | "--help", ..] => writeln!(out, "{USAGE}"),
        // What NATIVE runs, on the browser's pipes
        ["connect"] => return connect(input, out),
        ["status"] => {
            let names: Vec<String> = browsers().iter().rev().map(|b| b.name.clone()).collect();
            writeln!(
                out,
                "{}",
                json!({"browsers": names, "serves": "localhost", "allow": []})
            )
        }
        ["install", "firefox"] => writeln!(out, "installed {}", install_firefox()?.display()),
        ["install", browser] => {
            return Err(failed(format!("can't install for {browser}, only firefox")));
        }
        [command @ ("context" | "selection" | "read")] => print(out, &ask(command, &[])?),
        ["dom", selector @ ..] if selector.len() <= 1 => print(out, &ask("dom", selector)?),
        _ => {
            return Err(Error::Usage(
                "unknown command or wrong arguments".into(),
                USAGE,
            ));
        }
    };
    written.map_err(|e| failed(format!("can't answer: {e}")))
}
