//! local:, the inference of INFERENCE.md this host runs itself: speech
//! models through transcribe.cpp, the GGUF files in the Hugging Face cache
//! and in the human's model directories whose metadata has stt. keys, the
//! ones transcribe.cpp writes. The service serves it as a tree for
//! hunch-host to mount.

mod gguf;

use crate::error::{Error, Result, failed};
use crate::wav;
use hunch::p9::errno;
use hunch::tree::{self, ANSWER, Kind, READ, Tree, no_such};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The prefix of the metadata keys transcribe.cpp writes
const STT: &str = "stt.";
/// Deep enough for publisher/model/file, as LM Studio lays models out
const MAX_DEPTH: usize = 4;

pub struct Model {
  pub id: String,
  pub name: Option<String>,
  path: PathBuf,
  size: u64,
}

impl Model {
  pub fn to_json(&self) -> Value {
    json!({
        "id": self.id,
        "name": self.name,
        "inputs": [wav::TYPE],
        "outputs": ["text/plain"],
        "loaded": loaded_id().as_deref() == Some(self.id.as_str()),
        "size": self.size,
    })
  }
}

fn home() -> PathBuf {
  PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

fn hub() -> PathBuf {
  std::env::var_os("HF_HOME")
    .map(PathBuf::from)
    .unwrap_or_else(|| home().join(".cache/huggingface"))
    .join("hub")
}

fn model_dirs() -> Vec<PathBuf> {
  let config = std::env::var_os("XDG_CONFIG_HOME")
    .map(PathBuf::from)
    .unwrap_or_else(|| home().join(".config"));
  std::fs::read_to_string(config.join("hunch/models"))
    .unwrap_or_default()
    .lines()
    .map(str::trim)
    .filter(|line| !line.is_empty())
    .map(PathBuf::from)
    .collect()
}

/// A speech model's name, or None if the file isn't one
fn speech(path: &Path) -> Option<Option<String>> {
  let metadata = gguf::metadata(path).ok()?;
  metadata
    .iter()
    .any(|(key, _)| key.starts_with(STT))
    .then(|| {
      metadata
        .into_iter()
        .find(|(key, _)| key == "general.name")
        .and_then(|(_, name)| name)
    })
}

fn ggufs(dir: &Path, depth: usize) -> Vec<PathBuf> {
  let Ok(entries) = std::fs::read_dir(dir) else {
    return vec![];
  };
  let mut found: Vec<PathBuf> = entries
    .flatten()
    .flat_map(|entry| {
      let path = entry.path();
      if path.is_dir() && depth > 0 {
        ggufs(&path, depth - 1)
      } else if path.extension().is_some_and(|e| e == "gguf") {
        vec![path]
      } else {
        vec![]
      }
    })
    .collect();
  found.sort();
  found
}

fn model(id: String, path: PathBuf) -> Option<Model> {
  let name = speech(&path)?;
  let size = std::fs::metadata(&path).ok()?.len();
  Some(Model {
    id,
    name,
    path,
    size,
  })
}

/// The quantization a GGUF file name ends with, e.g. Q8_0
fn quant(path: &Path) -> String {
  let stem = path.file_stem().unwrap_or_default().to_string_lossy();
  stem.rsplit('-').next().unwrap_or(&stem).to_string()
}

/// The snapshot refs/main names, or, for a repo fetched at a pinned
/// revision as Handy does, each one it has
fn snapshots(repo: &Path) -> Vec<PathBuf> {
  let snapshots = repo.join("snapshots");
  if let Ok(main) = std::fs::read_to_string(repo.join("refs/main")) {
    return vec![snapshots.join(main.trim())];
  }
  let mut all: Vec<PathBuf> = std::fs::read_dir(&snapshots)
    .map(|entries| entries.flatten().map(|e| e.path()).collect())
    .unwrap_or_default();
  all.sort();
  all
}

/// The models of the Hugging Face cache: a repo's id, with :QUANT where it
/// has several
fn hub_models() -> Vec<Model> {
  let Ok(repos) = std::fs::read_dir(hub()) else {
    return vec![];
  };
  let mut models = vec![];
  for repo in repos.flatten() {
    let dir = repo.file_name().to_string_lossy().into_owned();
    let Some(id) = dir
      .strip_prefix("models--")
      .map(|r| r.replacen("--", "/", 1))
    else {
      continue;
    };
    let speech: Vec<PathBuf> = snapshots(&repo.path())
      .iter()
      .flat_map(|snapshot| ggufs(snapshot, MAX_DEPTH))
      .filter(|path| self::speech(path).is_some())
      .collect();
    let several = speech.len() > 1;
    models.extend(speech.into_iter().filter_map(|path| {
      let id = if several {
        format!("{id}:{}", quant(&path))
      } else {
        id.clone()
      };
      model(id, path)
    }));
  }
  models
}

pub fn models() -> Vec<Model> {
  let mut models = hub_models();
  for dir in model_dirs() {
    models.extend(ggufs(&dir, MAX_DEPTH).into_iter().filter_map(|path| {
      let id = path.strip_prefix(&dir).ok()?.to_string_lossy().into_owned();
      model(id, path)
    }));
  }
  models.sort_by(|a, b| a.id.cmp(&b.id));
  models
}

pub fn find(id: &str) -> Result<Model> {
  models()
    .into_iter()
    .find(|m| m.id == id)
    .ok_or_else(|| Error::Unknown(format!("no model {id}")))
}

/// The model in memory, kept so the next run doesn't wait for it to load
struct Loaded {
  id: String,
  session: transcribe_cpp::Session,
}

static LOADED: Mutex<Option<Loaded>> = Mutex::new(None);
/// The id of the model in LOADED, apart from it, since a run holds LOADED
/// for as long as it takes
static LOADED_ID: Mutex<Option<String>> = Mutex::new(None);

pub fn loaded_id() -> Option<String> {
  LOADED_ID.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn ensure_loaded(loaded: &mut Option<Loaded>, id: &str) -> Result<()> {
  if loaded.as_ref().is_some_and(|l| l.id == id) {
    return Ok(());
  }
  // ggml logs every kernel it compiles; failures come back as errors
  static QUIET: std::sync::Once = std::sync::Once::new();
  QUIET.call_once(transcribe_cpp::disable_logging);
  let model = find(id)?;
  let session = transcribe_cpp::Model::load(&model.path)
    .and_then(|m| m.session())
    .map_err(|e| failed(format!("can't load {id}: {e}")))?;
  *loaded = Some(Loaded {
    id: id.into(),
    session,
  });
  *LOADED_ID.lock().unwrap_or_else(|e| e.into_inner()) = Some(id.into());
  Ok(())
}

/// Loads the model ahead, so the first run doesn't wait for it
pub fn load(id: &str) -> Result<()> {
  ensure_loaded(&mut LOADED.lock().unwrap_or_else(|e| e.into_inner()), id)
}

/// Frees the model; ggml's Metal backend aborts at exit while one is loaded
pub fn unload() {
  LOADED.lock().unwrap_or_else(|e| e.into_inner()).take();
  LOADED_ID.lock().unwrap_or_else(|e| e.into_inner()).take();
}

/// The text of pcm, 16 kHz mono samples in [-1, 1], by the model id
pub fn transcribe(id: &str, pcm: &[f32]) -> Result<String> {
  let mut loaded = LOADED.lock().unwrap_or_else(|e| e.into_inner());
  ensure_loaded(&mut loaded, id)?;
  let session = &mut loaded.as_mut().expect("loaded above").session;
  let transcript = session
    .run(pcm, &transcribe_cpp::RunOptions::default())
    .map_err(|e| failed(format!("{id} failed: {e}")))?;
  Ok(transcript.text.trim().to_string())
}

/// The files of INFERENCE.md: models, and transcribe, which takes a line
/// of JSON, then a WAV
pub struct Local;

const FILES: [(&str, Kind); 2] = [("models", READ), ("transcribe", ANSWER)];

fn tree_error(e: Error) -> tree::Error {
  let errno = match e {
    Error::Unknown(_) | Error::Usage(..) => errno::EINVAL,
    _ => errno::EIO,
  };
  tree::Error::new(errno, e.to_string())
}

impl Local {
  fn transcribe(&self, input: &[u8]) -> Result<String> {
    let end = input
      .iter()
      .position(|&b| b == b'\n')
      .ok_or_else(|| failed("transcribe takes a line of JSON first"))?;
    let request: Value = serde_json::from_slice(&input[..end])
      .map_err(|e| failed(format!("the request isn't JSON: {e}")))?;
    let (Some(id), Some(kind)) =
      (request["model"].as_str(), request["type"].as_str())
    else {
      return Err(failed("the request has no model or no type"));
    };
    if kind != wav::TYPE {
      return Err(Error::Unknown(format!("{id} doesn't take {kind}")));
    }
    let pcm = wav::parse(&input[end + 1..]).map_err(failed)?;
    transcribe(id, &pcm)
  }
}

impl Tree for Local {
  fn kind(&self, _activity: &str, path: &[&str]) -> tree::Result<Kind> {
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

  fn list(
    &self,
    activity: &str,
    path: &[&str],
  ) -> tree::Result<Vec<(String, Kind)>> {
    if self.kind(activity, path)? != Kind::Dir {
      return Err(tree::Error::new(errno::ENOTDIR, "not a directory"));
    }
    Ok(FILES.iter().map(|(n, k)| (n.to_string(), *k)).collect())
  }

  fn read(
    &self,
    _activity: &str,
    path: &[&str],
    input: Option<&[u8]>,
  ) -> tree::Result<Vec<u8>> {
    match path {
      ["models"] => {
        let models: Vec<Value> = models().iter().map(Model::to_json).collect();
        Ok(Value::from(models).to_string().into_bytes())
      }
      ["transcribe"] => self
        .transcribe(input.unwrap_or_default())
        .map(String::into_bytes)
        .map_err(tree_error),
      _ => Err(no_such()),
    }
  }

  fn write(
    &self,
    _activity: &str,
    path: &[&str],
    _data: &[u8],
  ) -> tree::Result<()> {
    Err(tree::Error::new(
      errno::EACCES,
      format!("{}: only read it", path.join("/")),
    ))
  }
}

/// Serves local: at ROOT/sock/inference.sock, on threads of its own, for
/// as long as the service runs; the service keeps the model loaded
pub fn serve() {
  let socket = hunch::host::root::inference();
  // The service runs once, so a socket here is an earlier run's
  let _ = std::fs::remove_file(&socket);
  let listener = match std::os::unix::net::UnixListener::bind(&socket) {
    Ok(listener) => listener,
    Err(e) => {
      return eprintln!(
        "hunch-seat: can't serve local: at {}: {e}",
        socket.display()
      );
    }
  };
  use std::os::unix::fs::{MetadataExt, PermissionsExt};
  let _ =
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600));
  let uid = std::fs::metadata(hunch::host::root::root()).map_or(0, |m| m.uid());
  std::thread::spawn(move || {
    hunch::host::serve(listener, std::sync::Arc::new(Local), uid)
  });
}
