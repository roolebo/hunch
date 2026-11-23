//! The browser as files: BROWSER.md's Files, which hunch-browser-native
//! serves; the extension does the work.

use super::buffers::{Buffers, Holder, Numbers};
use crate::p9::errno;
use crate::registry::{Handler, Registry};
use crate::tree::{ANSWER, Error, Kind, Result, Tree, no_such};
use serde_json::Value;
use std::ffi::OsStr;
use std::sync::Arc;

/// The browser REGISTRY has
pub fn browser(registry: Arc<dyn Registry>) -> Buffers {
  let holder = Arc::new(Browser { registry });
  let numbers = Arc::new(Numbers::default());
  let dom = Dom {
    holder: holder.clone(),
    numbers: numbers.clone(),
  };
  Buffers::new(holder, numbers, Box::new(dom))
}

struct Browser {
  registry: Arc<dyn Registry>,
}

impl Browser {
  /// The extension's context, its TAB as the buffer
  fn context_json(&self, device: &dyn Handler) -> Result<Value> {
    let context = device.run(&["context".as_ref()])?;
    let mut context: Value = serde_json::from_slice(&context).map_err(|e| {
      Error::new(errno::EIO, format!("the browser's context isn't JSON: {e}"))
    })?;
    if let Some(fields) = context.as_object_mut() {
      let tab = fields.remove("tab").unwrap_or(Value::Null);
      fields.insert("buffer".into(), tab);
    }
    Ok(context)
  }

  /// The active TAB, as a BUFFER
  fn active(&self, device: &dyn Handler) -> Result<Option<String>> {
    Ok(match &self.context_json(device)?["buffer"] {
      Value::Null => None,
      Value::String(tab) => Some(tab.clone()),
      tab => Some(tab.to_string()),
    })
  }

  /// Fails unless BUFFER is the active tab, the only one whose content
  /// leaves the browser
  fn check_active(&self, device: &dyn Handler, buffer: &str) -> Result<()> {
    if self.active(device)?.as_deref() == Some(buffer) {
      return Ok(());
    }
    Err(Error::new(
      errno::ENOENT,
      format!("tab {buffer} isn't the active one, the only one to read"),
    ))
  }
}

impl Holder for Browser {
  fn device(&self, _activity: &str) -> Result<Box<dyn Handler>> {
    self
      .registry
      .find("browser", None)
      .ok_or_else(|| Error::new(errno::ENOENT, "no browser"))
  }

  fn context(&self, device: &dyn Handler) -> Result<Vec<u8>> {
    Ok(format!("{}\n", self.context_json(device)?).into_bytes())
  }

  fn buffers(&self, device: &dyn Handler) -> Result<Vec<String>> {
    Ok(self.active(device)?.into_iter().collect())
  }

  fn body(&self, device: &dyn Handler, buffer: &str) -> Result<Vec<u8>> {
    self.check_active(device, buffer)?;
    device.run(&["read".as_ref()])
  }
}

/// The browser's own file: each buffer's dom
struct Dom {
  holder: Arc<Browser>,
  numbers: Arc<Numbers>,
}

impl Dom {
  fn buffer(&self, activity: &str, n: &str) -> Result<String> {
    let browser = self.holder.device(activity)?;
    self.numbers.buffer(browser.as_ref(), n).ok_or_else(no_such)
  }
}

impl Tree for Dom {
  fn kind(&self, activity: &str, path: &[&str]) -> Result<Kind> {
    match path {
      [n, "dom"] => self.buffer(activity, n).map(|_| ANSWER),
      _ => Err(no_such()),
    }
  }

  fn list(&self, activity: &str, path: &[&str]) -> Result<Vec<(String, Kind)>> {
    match path {
      [] => Ok(Vec::new()),
      [n] => self
        .buffer(activity, n)
        .map(|_| vec![("dom".into(), ANSWER)]),
      _ => Err(no_such()),
    }
  }

  fn read(
    &self,
    activity: &str,
    path: &[&str],
    input: Option<&[u8]>,
  ) -> Result<Vec<u8>> {
    let [n, "dom"] = path else {
      return Err(no_such());
    };
    let id = self.buffer(activity, n)?;
    let browser = self.holder.device(activity)?;
    self.holder.check_active(browser.as_ref(), &id)?;
    let selector = input.map(String::from_utf8_lossy);
    let selector = selector.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let mut args: Vec<&OsStr> = vec!["dom".as_ref()];
    args.extend(selector.map(OsStr::new));
    browser.run(&args)
  }

  fn write(&self, _activity: &str, path: &[&str], _data: &[u8]) -> Result<()> {
    Err(Error::new(
      errno::EACCES,
      format!("{}: only read it", path.join("/")),
    ))
  }
}
