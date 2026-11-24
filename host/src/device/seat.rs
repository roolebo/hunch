//! The seat as files: SEAT.md's Files. hunch-seat does the work; Current
//! decides which activity sees it.

use super::buffers::{Buffers, Holder, Numbers};
use crate::p9::errno;
use crate::registry::Handler;
use crate::tree::no_such;
use crate::tree::{Error, Kind, READ, Result, Tree, Union, WRITE, Workspace};
use serde_json::Value;
use std::ffi::OsStr;
use std::sync::Arc;

const FILES: [(&str, Kind); 6] = [
  ("context", READ),
  ("last", READ),
  ("menu", READ),
  ("devices", READ),
  ("needs", READ),
  ("ctl", WRITE),
];

/// The seat whose commands SEAT runs, e.g. hunch-seat seat, with its wm
pub fn seat(seat: Box<dyn Handler>) -> Union {
  let seat: Arc<dyn Handler> = Arc::from(seat);
  let wm = Wm {
    seat: seat.clone(),
    numbers: Arc::new(Numbers::default()),
    inside: None,
  };
  Union::new(vec![
    Box::new(Seat { seat }),
    Box::new(Workspace::new(vec![("wm", Box::new(wm.buffers()))])),
  ])
}

struct Seat {
  seat: Arc<dyn Handler>,
}

/// A wm of the seat, through the seat's command wm: the seat's own, or
/// one that runs in a window of another
struct Wm {
  seat: Arc<dyn Handler>,
  /// Of every wm's windows
  numbers: Arc<Numbers>,
  /// The ADDR of a wm in a window, and the CLIENT that shows it there
  inside: Option<(String, String)>,
}

impl Wm {
  /// Its windows as buffers, with no files of its own
  fn buffers(self) -> Buffers {
    let numbers = self.numbers.clone();
    Buffers::new(Arc::new(self), numbers, Box::new(Workspace::new(vec![])))
  }

  fn run(&self, args: &[&str]) -> Result<Vec<u8>> {
    let mut all = vec!["wm"];
    all.extend(self.inside.as_ref().map(|(addr, _)| addr.as_str()));
    all.extend(args);
    let all: Vec<&OsStr> = all.into_iter().map(OsStr::new).collect();
    self.seat.run(&all)
  }

  fn json(&self, args: &[&str]) -> Result<Value> {
    serde_json::from_slice(&self.run(args)?).map_err(|e| {
      let command = args[0];
      Error::new(errno::EIO, format!("the wm's {command} isn't JSON: {e}"))
    })
  }

  /// Where the human is in it; in a wm inside a window, on its CLIENT
  fn context_json(&self) -> Result<Value> {
    match &self.inside {
      Some((_, client)) => self.json(&["context", "--client", client]),
      None => self.json(&["context"]),
    }
  }
}

/// The seat, by the name of a wm, so each wm's windows are numbered apart
struct Named {
  seat: Arc<dyn Handler>,
  name: String,
}

impl Handler for Named {
  fn run(&self, args: &[&OsStr]) -> Result<Vec<u8>> {
    self.seat.run(args)
  }

  fn addr(&self) -> &str {
    &self.name
  }
}

impl Holder for Wm {
  fn device(&self, _activity: &str) -> Result<Box<dyn Handler>> {
    let name = match &self.inside {
      Some((addr, client)) => format!("wm {addr} {client}"),
      None => "wm".into(),
    };
    let seat = self.seat.clone();
    Ok(Box::new(Named { seat, name }))
  }

  /// The wm's, its focused window as the buffer; nothing is selected
  fn context(&self, _seat: &dyn Handler) -> Result<Vec<u8>> {
    let mut context = self.context_json()?;
    context["buffer"] = context["window"]["id"].clone();
    context["selection"] = Value::Null;
    Ok(format!("{context}\n").into_bytes())
  }

  fn selection(&self, _seat: &dyn Handler) -> Result<Vec<u8>> {
    Ok(Vec::new())
  }

  /// Its windows; in a wm inside a window, those its CLIENT shows
  fn buffers(&self, _seat: &dyn Handler) -> Result<Vec<String>> {
    let shown = match self.inside {
      Some(_) => Some(self.context_json()?["workspace"]["id"].clone()),
      None => None,
    };
    let windows = self.json(&["windows"])?;
    let windows = windows.as_array().into_iter().flatten();
    Ok(
      windows
        .filter(|w| shown.as_ref().is_none_or(|s| w["workspace"] == *s))
        .filter_map(|w| w["id"].as_str().map(String::from))
        .collect(),
    )
  }

  /// What the window shows, as PNG
  fn body(&self, _seat: &dyn Handler, window: &str) -> Result<Vec<u8>> {
    self.run(&["screen", window])
  }

  /// The wm that runs in the window, as a tmux attached in it
  fn inner(
    &self,
    _seat: &dyn Handler,
    window: &str,
  ) -> Result<Option<Box<dyn Tree>>> {
    let inside = self.json(&["inside", window])?;
    let (Some(addr), Some(client)) =
      (inside["addr"].as_str(), inside["client"].as_str())
    else {
      return Ok(None);
    };
    let wm = Wm {
      seat: self.seat.clone(),
      numbers: self.numbers.clone(),
      inside: Some((addr.into(), client.into())),
    };
    Ok(Some(Box::new(wm.buffers())))
  }
}

impl Tree for Seat {
  fn kind(&self, _activity: &str, path: &[&str]) -> Result<Kind> {
    match path {
      [] => Ok(Kind::Dir),
      [name] => FILES
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, k)| *k)
        .ok_or_else(no_such),
      _ => Err(no_such()),
    }
  }

  fn list(&self, activity: &str, path: &[&str]) -> Result<Vec<(String, Kind)>> {
    if self.kind(activity, path)? != Kind::Dir {
      return Err(Error::new(
        errno::ENOTDIR,
        format!("{}: not a directory", path.join("/")),
      ));
    }
    Ok(FILES.iter().map(|(n, k)| (n.to_string(), *k)).collect())
  }

  fn read(
    &self,
    _activity: &str,
    path: &[&str],
    _input: Option<&[u8]>,
  ) -> Result<Vec<u8>> {
    // Each file to read is the seat's command of its name
    match path {
      [name] if FILES.contains(&(*name, READ)) => {
        self.seat.run(&[name.as_ref()])
      }
      _ => Err(no_such()),
    }
  }

  fn write(&self, _activity: &str, path: &[&str], data: &[u8]) -> Result<()> {
    match path {
      ["ctl"] => self.ctl(data),
      _ => Err(Error::new(
        errno::EACCES,
        format!("{}: only read it", path.join("/")),
      )),
    }
  }
}

impl Seat {
  fn ctl(&self, data: &[u8]) -> Result<()> {
    let text = String::from_utf8_lossy(data);
    let lines: Vec<&str> =
      text.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
      return Err(Error::new(
        errno::EINVAL,
        "ctl takes a command, e.g. target focus",
      ));
    }
    for line in lines {
      let args: Vec<&OsStr> = line.split_whitespace().map(OsStr::new).collect();
      self.seat.run(&args)?;
    }
    Ok(())
  }
}
