//! The compute role, as skills/hunch/COMPUTE.md specifies it. local: runs
//! speech models through transcribe.cpp: the GGUF files in the Hugging Face
//! cache and in the human's model directories whose metadata has stt.
//! keys, the ones transcribe.cpp writes.

mod gguf;

use crate::error::{Error, Result, failed};
use crate::wav;
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const USAGE: &str = "\
Usage: hunch-host compute ADDR COMMAND [ARGUMENTS]

Commands:
  models          The models it can run, as JSON
  run MODEL TYPE  Run MODEL on stdin, print its answer
  load MODEL      Load MODEL now, so the next run doesn't wait for it
  unload          Free the loaded model's memory

ADDR is local: so far. TYPE is audio/wav so far: 16-bit PCM, 16 kHz, mono.";

const LOCAL: &str = "local:";
const WAV: &str = "audio/wav";
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
    fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "inputs": [WAV],
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

/// Runs a compute command on the caller's input and output, in the
/// service, which keeps the model loaded
pub fn run(args: &[String], input: &mut dyn Read, out: &mut dyn Write) -> Result<()> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let write = |out: &mut dyn Write, text: &dyn std::fmt::Display| {
        writeln!(out, "{text}").map_err(|e| failed(format!("can't write the answer: {e}")))
    };
    match args.as_slice() {
        ["-h" | "--help", ..] => write(out, &USAGE)?,
        [LOCAL, "models"] => {
            let models: Vec<Value> = models().iter().map(Model::to_json).collect();
            write(out, &Value::from(models))?;
        }
        [LOCAL, "run", id, kind] => {
            if *kind != WAV {
                return Err(Error::Unknown(format!("{id} doesn't take {kind}")));
            }
            let mut wav = vec![];
            input
                .read_to_end(&mut wav)
                .map_err(|e| failed(format!("can't read INPUT: {e}")))?;
            let pcm = wav::parse(&wav).map_err(failed)?;
            write(out, &transcribe(id, &pcm)?)?;
        }
        [LOCAL, "load", id] => {
            load(id)?;
            write(out, &format!("loaded {id}"))?;
        }
        [LOCAL, "unload"] => {
            unload();
            write(out, &"unloaded")?;
        }
        [addr, ..] if *addr != LOCAL => {
            return Err(failed(format!(
                "this build has no compute at {addr}, only local:"
            )));
        }
        _ => {
            return Err(Error::Usage(
                "unknown command or wrong arguments".into(),
                USAGE,
            ));
        }
    }
    Ok(())
}
