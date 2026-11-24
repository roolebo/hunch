//! The editor of an activity as files, after acme: PROTOCOL.md's Files.
//! A device of PROTOCOL.md does the work.

use super::buffers::{Buffers, Holder, Numbers};
use crate::p9::errno;
use crate::registry::{Handler, Registry};
use crate::tree::{ANSWER, Error, Kind, Result, Tree, WRITE, no_such};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// A command written to ctl: one argument per line, so a FILE may have
/// spaces
#[derive(Debug, PartialEq)]
enum Ctl {
  Open(Vec<String>),
  Reload,
  Review(String, String),
}

impl Ctl {
  fn parse(data: &[u8]) -> Result<Ctl> {
    let text = String::from_utf8_lossy(data);
    let args: Vec<&str> = text.lines().collect();
    let usage = || {
      Error::new(
        errno::EINVAL,
        "ctl takes, one per line: open, FILE[, LINE[, COL]] | reload | \
         review, REPO, RANGE",
      )
    };
    match args.as_slice() {
      ["open", rest @ ..] if (1..=3).contains(&rest.len()) => {
        if rest[1..].iter().any(|n| n.parse::<u32>().is_err()) {
          return Err(usage());
        }
        Ok(Ctl::Open(rest.iter().map(|s| s.to_string()).collect()))
      }
      ["reload"] => Ok(Ctl::Reload),
      ["review", repo, range] => {
        Ok(Ctl::Review(repo.to_string(), range.to_string()))
      }
      _ => Err(usage()),
    }
  }

  fn args(&self) -> Vec<&str> {
    match self {
      Ctl::Open(args) => std::iter::once("open")
        .chain(args.iter().map(String::as_str))
        .collect(),
      Ctl::Reload => vec!["reload"],
      Ctl::Review(repo, range) => vec!["review", repo, range],
    }
  }
}

/// A text on its way to the editor, removed once it's there
struct Text(PathBuf);

impl Text {
  fn new(dir: &Path, data: &[u8]) -> Result<Text> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let path = dir.join(format!("{}-{n}", std::process::id()));
    fs::write(&path, data).map_err(|e| {
      Error::new(errno::EIO, format!("can't write {}: {e}", path.display()))
    })?;
    Ok(Text(path))
  }
}

impl Drop for Text {
  fn drop(&mut self) {
    let _ = fs::remove_file(&self.0);
  }
}

/// PROTOCOL.md's read prints each line after its number and a tab
fn unnumber(numbered: &[u8]) -> Vec<u8> {
  let mut text = Vec::new();
  for line in String::from_utf8_lossy(numbered).lines() {
    text.extend_from_slice(
      line.split_once('\t').map_or(line, |(_, l)| l).as_bytes(),
    );
    text.push(b'\n');
  }
  text
}

fn without_newline(data: &[u8]) -> String {
  let text = String::from_utf8_lossy(data);
  text.strip_suffix('\n').unwrap_or(&text).to_string()
}

/// The editor of an activity, from REGISTRY, with the texts it takes in
/// files in TMP
pub fn editor(registry: Arc<dyn Registry>, tmp: PathBuf) -> Buffers {
  let holder = Arc::new(Editor { registry });
  let numbers = Arc::new(Numbers::default());
  let edits = Edits {
    holder: holder.clone(),
    numbers: numbers.clone(),
    tmp,
    addr: Mutex::default(),
  };
  Buffers::new(holder, numbers, Box::new(edits))
}

struct Editor {
  registry: Arc<dyn Registry>,
}

impl Holder for Editor {
  fn device(&self, activity: &str) -> Result<Box<dyn Handler>> {
    self.registry.find("editor", Some(activity)).ok_or_else(|| {
      Error::new(
        errno::ENOENT,
        format!(
          "no editor in activity {activity}: start one in it with hunch support"
        ),
      )
    })
  }

  fn buffers(&self, device: &dyn Handler) -> Result<Vec<String>> {
    let context = self.context(device)?;
    let context: serde_json::Value =
      serde_json::from_slice(&context).map_err(|e| {
        Error::new(errno::EIO, format!("the editor's context isn't JSON: {e}"))
      })?;
    let buffers = context["buffers"].as_array().into_iter().flatten();
    Ok(
      buffers
        .filter_map(|b| b["buffer"].as_str().map(String::from))
        .collect(),
    )
  }

  fn body(&self, device: &dyn Handler, buffer: &str) -> Result<Vec<u8>> {
    Ok(unnumber(&device.run(&["read".as_ref(), buffer.as_ref()])?))
  }
}

/// The editor's own files: ctl, new, and each buffer's addr and data
struct Edits {
  holder: Arc<Editor>,
  numbers: Arc<Numbers>,
  tmp: PathBuf,
  /// What addr holds, by the editor's address and N, until data takes it
  addr: Mutex<HashMap<(String, usize), Vec<u8>>>,
}

impl Edits {
  /// The N and BUFFER of N, if the owner gave N to one
  fn buffer(&self, editor: &dyn Handler, n: &str) -> Option<(usize, String)> {
    let id = self.numbers.buffer(editor, n)?;
    Some((n.parse().ok()?, id))
  }
}

impl Tree for Edits {
  fn kind(&self, activity: &str, path: &[&str]) -> Result<Kind> {
    let editor = self.holder.device(activity)?;
    match path {
      ["ctl"] => Ok(WRITE),
      ["new"] => Ok(ANSWER),
      [n, "addr" | "data"] if self.buffer(editor.as_ref(), n).is_some() => {
        Ok(WRITE)
      }
      _ => Err(no_such()),
    }
  }

  fn list(&self, activity: &str, path: &[&str]) -> Result<Vec<(String, Kind)>> {
    let editor = self.holder.device(activity)?;
    let files: &[(&str, Kind)] = match path {
      [] => &[("ctl", WRITE), ("new", ANSWER)],
      [n] if self.buffer(editor.as_ref(), n).is_some() => {
        &[("addr", WRITE), ("data", WRITE)]
      }
      _ => return Err(no_such()),
    };
    Ok(files.iter().map(|(n, k)| (n.to_string(), *k)).collect())
  }

  fn read(
    &self,
    activity: &str,
    path: &[&str],
    input: Option<&[u8]>,
  ) -> Result<Vec<u8>> {
    let editor = self.holder.device(activity)?;
    match path {
      ["new"] => {
        let id = input
          .map(without_newline)
          .filter(|id| !id.is_empty())
          .ok_or_else(|| {
            Error::new(errno::EINVAL, "new: write a BUFFER first")
          })?;
        let n = self.numbers.number(editor.as_ref(), &id);
        Ok(format!("{n}\n").into_bytes())
      }
      _ => Err(Error::new(
        errno::EACCES,
        format!("{}: only write it", path.join("/")),
      )),
    }
  }

  fn write(&self, activity: &str, path: &[&str], data: &[u8]) -> Result<()> {
    let editor = self.holder.device(activity)?;
    match path {
      ["ctl"] => {
        let ctl = Ctl::parse(data)?;
        let args: Vec<&OsStr> =
          ctl.args().into_iter().map(OsStr::new).collect();
        editor.run(&args).map(drop)
      }
      [n, file @ ("addr" | "data")] => {
        let (n, id) = self.buffer(editor.as_ref(), n).ok_or_else(no_such)?;
        let key = (editor.addr().to_string(), n);
        // A replace drops the final newline a heredoc gives its texts, so
        // they may end mid-line; a created file keeps it
        let line =
          |text: &[u8]| text.strip_suffix(b"\n").unwrap_or(text).to_vec();
        let mut addr = self.addr.lock().unwrap_or_else(|e| e.into_inner());
        if *file == "addr" {
          addr.insert(key, line(data));
          return Ok(());
        }
        let old = addr.remove(&key);
        drop(addr);
        match old {
          Some(old) => {
            let old = Text::new(&self.tmp, &old)?;
            let new = Text::new(&self.tmp, &line(data))?;
            let args = [
              "replace".as_ref(),
              id.as_ref(),
              old.0.as_os_str(),
              new.0.as_os_str(),
            ];
            editor.run(&args).map(drop)
          }
          None => {
            let new = Text::new(&self.tmp, data)?;
            editor
              .run(&["create".as_ref(), id.as_ref(), new.0.as_os_str()])
              .map(drop)
          }
        }
      }
      _ => Err(no_such()),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn ctl_takes_one_argument_per_line() {
    assert_eq!(
      Ctl::parse(b"open\n/a b.c\n4\n").unwrap(),
      Ctl::Open(vec!["/a b.c".into(), "4".into()])
    );
    assert_eq!(Ctl::parse(b"reload\n").unwrap(), Ctl::Reload);
    assert_eq!(
      Ctl::parse(b"review\n/repo\nmain..topic").unwrap().args(),
      ["review", "/repo", "main..topic"]
    );
    for bad in [
      &b"open"[..],
      b"open\n/a\nfour",
      b"reload\nnow",
      b"close\n/a",
      b"",
    ] {
      assert_eq!(Ctl::parse(bad).unwrap_err().errno, errno::EINVAL);
    }
  }

  #[test]
  fn body_is_the_text_without_numbers() {
    assert_eq!(
      unnumber(b"     1\tfirst\n     2\t\tindented\n"),
      b"first\n\tindented\n"
    );
    assert_eq!(unnumber(b""), b"");
  }
}
