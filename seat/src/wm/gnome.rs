//! GNOME's desktop, through hunch's Shell extension: on Wayland, only the
//! Shell sees and types into its windows. README.md's GNOME says how the
//! extension serves it.

use super::{Context, Display, Frame, Pointer, Window, Wm, Workspace};
use crate::error::{Result, failed};
use hunch::peer::Peer;
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const ADDR: &str = "gnome:";
/// How long the human has to pick a window
const PICK_WAIT: Duration = Duration::from_secs(300);

/// The extension, while it serves the desktop
static SHELL: Mutex<Option<Arc<Peer>>> = Mutex::new(None);

fn shell() -> Result<Arc<Peer>> {
  SHELL
    .lock()
    .unwrap_or_else(|e| e.into_inner())
    .clone()
    .ok_or_else(|| {
      failed("GNOME Shell doesn't serve its desktop: enable hunch@hunch")
    })
}

pub fn served() -> bool {
  shell().is_ok()
}

/// Takes the extension that serves the desktop until it goes, on its pipes
pub fn serve(input: &mut File, out: &mut File) -> Result<()> {
  let out = out
    .try_clone()
    .map_err(|e| failed(format!("can't keep GNOME Shell's output: {e}")))?;
  let peer = Peer::new("GNOME Shell", Box::new(out));
  *SHELL.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::clone(&peer));
  let mut read = Ok(());
  for line in BufReader::new(input).lines() {
    let line = match line {
      Ok(line) => line,
      Err(e) => {
        read = Err(failed(format!("can't read GNOME Shell's extension: {e}")));
        break;
      }
    };
    match serde_json::from_str::<Value>(&line) {
      Ok(message) => peer.answer(&message),
      Err(e) => {
        eprintln!("hunch-seat: GNOME Shell sent {line:?}, not JSON: {e}")
      }
    }
  }
  let mut shell = SHELL.lock().unwrap_or_else(|e| e.into_inner());
  // Unless a newer one took its place, as after a Shell restart
  if shell.as_ref().is_some_and(|s| Arc::ptr_eq(s, &peer)) {
    *shell = None;
  }
  peer.close();
  read
}

fn text(value: &Value) -> Option<String> {
  value.as_str().map(String::from)
}

fn number(value: &Value) -> f64 {
  value.as_f64().unwrap_or_default()
}

fn frame(value: &Value) -> Frame {
  Frame {
    x: number(&value["x"]),
    y: number(&value["y"]),
    width: number(&value["width"]),
    height: number(&value["height"]),
  }
}

fn window(value: &Value) -> Window {
  Window {
    id: text(&value["id"]).unwrap_or_default(),
    app: text(&value["app"]).unwrap_or_default(),
    title: text(&value["title"]).unwrap_or_default(),
    pid: value["pid"].as_i64(),
    tty: text(&value["tty"]),
    host: text(&value["host"]),
    workspace: text(&value["workspace"]),
    frame: frame(value),
    visible: value["visible"] == true,
    state: value["state"].as_str().and_then(|s| s.parse().ok()),
    focused: value["focused"] == true,
  }
}

fn list(value: Value) -> Vec<Value> {
  match value {
    Value::Array(items) => items,
    _ => vec![],
  }
}

pub struct Gnome;

impl Wm for Gnome {
  fn scheme(&self) -> &'static str {
    "gnome"
  }

  fn context(&self, _client: Option<&str>) -> Result<Context> {
    let context = shell()?.ask("context", &[], None)?;
    let workspace = &context["workspace"];
    let pointer = &context["pointer"];
    Ok(Context {
      wm: "gnome",
      workspace: workspace.is_object().then(|| Workspace {
        id: text(&workspace["id"]),
        name: text(&workspace["name"]),
        display: text(&workspace["display"]),
      }),
      window: context["window"]
        .is_object()
        .then(|| window(&context["window"])),
      pointer: pointer.is_object().then(|| Pointer {
        x: number(&pointer["x"]),
        y: number(&pointer["y"]),
        display: text(&pointer["display"]),
        window: text(&pointer["window"]),
      }),
    })
  }

  fn displays(&self) -> Result<Vec<Display>> {
    let displays = list(shell()?.ask("displays", &[], None)?);
    Ok(
      displays
        .iter()
        .map(|d| Display {
          id: text(&d["id"]).unwrap_or_default(),
          name: text(&d["name"]),
          frame: frame(d),
          main: d["main"] == true,
        })
        .collect(),
    )
  }

  fn windows(&self) -> Result<Vec<Window>> {
    Ok(
      list(shell()?.ask("windows", &[], None)?)
        .iter()
        .map(window)
        .collect(),
    )
  }

  fn send(&self, window: &str, text: &str) -> Result<()> {
    shell()?.ask("send", &[window], Some(text))?;
    Ok(())
  }

  fn submit(&self, window: &str) -> Result<()> {
    shell()?.ask("submit", &[window], None)?;
    Ok(())
  }

  fn focus(&self, window: &str) -> Result<()> {
    shell()?.ask("focus", &[window], None)?;
    Ok(())
  }

  fn pick(&self) -> Result<String> {
    let picked = shell()?.ask_for(PICK_WAIT, "pick", &[], None)?;
    text(&picked).ok_or_else(|| {
      failed(format!("GNOME Shell picked {picked}, not a window"))
    })
  }
}
