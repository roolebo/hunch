//! An OpenAI-compatible server as INFERENCE.md's files: its models, with
//! what they take and answer, and its audio/transcriptions, with the key
//! this host keeps for it.

use super::{Keys, openai};
use crate::p9::errno;
use crate::tree::{ANSWER, Error, Kind, READ, Result, Tree, WRITE, no_such};
use serde_json::Value;
use std::sync::Arc;

const FILES: [(&str, Kind); 3] =
  [("models", READ), ("transcribe", ANSWER), ("login", WRITE)];

pub struct Server {
  pub url: String,
  pub keys: Arc<dyn Keys>,
}

fn failed(message: impl Into<String>) -> Error {
  Error::new(errno::EIO, message)
}

fn invalid(message: impl Into<String>) -> Error {
  Error::new(errno::EINVAL, message)
}

impl Server {
  fn models(&self) -> Result<Vec<u8>> {
    let key = self.keys.lookup(&self.url);
    let listed = openai::models(&self.url, key.as_deref()).map_err(failed)?;
    let models: Vec<Value> = listed
      .iter()
      .filter_map(|m| Some(openai::describe(m["id"].as_str()?, m)))
      .collect();
    Ok(Value::from(models).to_string().into_bytes())
  }

  /// The text of the speech in INPUT: a line of JSON, then the speech
  fn transcribe(&self, input: &[u8]) -> Result<Vec<u8>> {
    let end = input
      .iter()
      .position(|&b| b == b'\n')
      .ok_or_else(|| invalid("transcribe takes a line of JSON first"))?;
    let request: Value = serde_json::from_slice(&input[..end])
      .map_err(|e| invalid(format!("the request isn't JSON: {e}")))?;
    let (Some(model), Some(kind)) =
      (request["model"].as_str(), request["type"].as_str())
    else {
      return Err(invalid("the request has no model or no type"));
    };
    let key = self.keys.lookup(&self.url);
    let speech = (kind, &input[end + 1..]);
    let text = openai::transcribe(&self.url, key.as_deref(), model, speech)
      .map_err(failed)?;
    Ok(text.into_bytes())
  }

  fn login(&self, data: &[u8]) -> Result<()> {
    let key = String::from_utf8_lossy(data).trim().to_string();
    if key.is_empty() {
      return Err(invalid("login takes the key"));
    }
    self.keys.store(&self.url, &key).map_err(failed)
  }
}

impl Tree for Server {
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
      return Err(Error::new(errno::ENOTDIR, "not a directory"));
    }
    Ok(FILES.iter().map(|(n, k)| (n.to_string(), *k)).collect())
  }

  fn read(
    &self,
    _activity: &str,
    path: &[&str],
    input: Option<&[u8]>,
  ) -> Result<Vec<u8>> {
    match path {
      ["models"] => self.models(),
      ["transcribe"] => self.transcribe(input.unwrap_or_default()),
      _ => Err(no_such()),
    }
  }

  fn write(&self, _activity: &str, path: &[&str], data: &[u8]) -> Result<()> {
    match path {
      ["login"] => self.login(data),
      _ => Err(Error::new(
        errno::EACCES,
        format!("{}: only read it", path.join("/")),
      )),
    }
  }
}
