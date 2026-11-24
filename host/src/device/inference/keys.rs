//! The keys this host logs in to ENDPOINTs with, kept in the OS's store
//! of secrets: the Keychain on macOS, the Secret Service on Linux. Each
//! host has its own, so it's revoked on its own, and none sits in a file.

type Result<T> = std::result::Result<T, String>;
use std::io::Write;
use std::process::{Command, Stdio};

/// The service the keys are kept under, each by its ENDPOINT
const SERVICE: &str = "hunch";

/// Runs PROGRAM with ARGS, INPUT on its stdin; its stdout
fn run(program: &str, args: &[&str], input: &str) -> Result<String> {
  let mut child = Command::new(program)
    .args(args)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .map_err(|e| format!("can't run {program}: {e}"))?;
  child
    .stdin
    .take()
    .expect("stdin is piped")
    .write_all(input.as_bytes())
    .map_err(|e| format!("can't hand {program} the key: {e}"))?;
  let out = child
    .wait_with_output()
    .map_err(|e| format!("{program} didn't finish: {e}"))?;
  if !out.status.success() {
    let said = String::from_utf8_lossy(&out.stderr);
    return Err(format!("{program}: {}", said.trim()));
  }
  Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Keeps KEY for ENDPOINT, in place of the one before
#[cfg(target_os = "macos")]
fn store(endpoint: &str, key: &str) -> Result<()> {
  let quote =
    |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
  // A command on stdin, so the key stays off the command line
  let command = format!(
    "add-generic-password -U -s {} -a {} -w {}\n",
    quote(SERVICE),
    quote(endpoint),
    quote(key)
  );
  run("security", &["-i"], &command).map(drop)
}

#[cfg(target_os = "linux")]
fn store(endpoint: &str, key: &str) -> Result<()> {
  let label = format!("hunch: {endpoint}");
  let args = [
    "store", "--label", &label, "service", SERVICE, "endpoint", endpoint,
  ];
  run("secret-tool", &args, key).map(drop)
}

/// The key for ENDPOINT, if this host keeps one
#[cfg(target_os = "macos")]
fn lookup(endpoint: &str) -> Option<String> {
  let args = ["find-generic-password", "-s", SERVICE, "-a", endpoint, "-w"];
  run("security", &args, "").ok().filter(|k| !k.is_empty())
}

#[cfg(target_os = "linux")]
fn lookup(endpoint: &str) -> Option<String> {
  let args = ["lookup", "service", SERVICE, "endpoint", endpoint];
  run("secret-tool", &args, "").ok().filter(|k| !k.is_empty())
}

/// The OS's store of secrets
pub struct OsKeys;

impl super::Keys for OsKeys {
  fn lookup(&self, endpoint: &str) -> Option<String> {
    lookup(endpoint)
  }

  fn store(&self, endpoint: &str, key: &str) -> Result<()> {
    store(endpoint, key)
  }
}
