//! The seat, as skills/hunch/SEAT.md specifies it, on macOS and on
//! Linux. It lives in the service, which holds the dictation key, the
//! recording and the loaded model, since they outlive any one command.

mod audio;

use crate::error::{Error, Result, failed};
use serde_json::{Value, json};
use std::fs::File;
use std::io::Write;
use std::sync::atomic::AtomicBool;

pub const USAGE: &str = "\
Usage: hunch-seat seat COMMAND [ARGUMENTS]

Commands:
  devices               The microphones, as JSON";

/// Answers a seat command in the service, writing to the client's stdout
pub fn run(
  args: &[String],
  _input: &mut File,
  out: &mut File,
  _gone: &AtomicBool,
) -> Result<()> {
  let args: Vec<&str> = args.iter().map(String::as_str).collect();
  let said = match args.as_slice() {
    ["-h" | "--help", ..] => USAGE.to_string(),
    ["devices"] => {
      let mics: Vec<Value> = audio::mics()
        .map_err(failed)?
        .into_iter()
        .map(|mic| {
          json!({
              "id": mic.id,
              "kind": "mic",
              "name": mic.name,
              "in_use": mic.in_use,
              // The seat doesn't watch the signal yet
              "signal": null,
          })
        })
        .collect();
      Value::from(mics).to_string()
    }
    _ => {
      return Err(Error::Usage(
        "unknown command or wrong arguments".into(),
        USAGE,
      ));
    }
  };
  writeln!(out, "{said}").map_err(|e| failed(format!("can't answer: {e}")))
}
