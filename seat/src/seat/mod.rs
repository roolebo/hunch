//! The seat, as skills/hunch/SEAT.md specifies it, on macOS and on
//! Linux; platform has each one's keys and presence.
//! It lives in the service, which holds the dictation key, the recording
//! and the loaded model, since they outlive any one command.

mod audio;
#[cfg(target_os = "linux")]
mod linux;

use crate::error::{Error, Result, failed};
#[cfg(feature = "inference")]
use crate::inference;
#[cfg(target_os = "macos")]
use crate::macos as platform;
use crate::{opus, wav, wm};
#[cfg(target_os = "linux")]
use linux as platform;
use platform::Key;
use serde_json::{Value, json};
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard, OnceLock, mpsc};
use std::thread;
use std::time::{Duration, Instant};

pub const USAGE: &str = "\
Usage: hunch-seat seat COMMAND [ARGUMENTS]

Commands:
  context               The seat's state, as JSON
  devices               The microphones, as JSON
  last                  The last transcription, kept in memory only
  needs                 Every grant the service uses, with whether it has
                        it, as needs --all prints them
  watch                 The state again each time it changes, a line each
  model MODEL           Transcribe with MODEL, one of inference/models
  load                  Load the model now
  eject                 Free the model's memory now
  submit on|off         Press Enter after the text, or not
  preload on|off        Load the model at start, or at the first dictation
  warm MINUTES|always   Keep the model in GPU memory this long while idle,
                        from the service's next start
  key KEY               Dictate while KEY is held: right-super,
                        right-alt, right-control, right-shift or fn

Dictation goes to the window in focus. Escape cancels it.";

#[cfg(target_os = "macos")]
const WM: &str = "macos:";
// The Shell extension serves it (see README.md)
#[cfg(target_os = "linux")]
const WM: &str = "gnome:";
const NO_MODEL: &str = "no model: pick one in hunch's settings";
#[cfg(feature = "inference")]
const WARM_VAR: &str = "GGML_METAL_RESIDENCY_KEEP_ALIVE_S";
/// Ten years, in seconds: as good as always, and within an int
#[cfg(feature = "inference")]
const WARM_ALWAYS: &str = "315360000";
/// Handy's default, which the human knows
const DEFAULT_KEY: &str = "right-super";
/// How often a watch checks whether its client is gone
const WATCH_CHECK: Duration = Duration::from_secs(1);
/// How long the seat waits for Accessibility before it watches keys
const KEYS_RETRY: Duration = Duration::from_secs(5);

/// What the seat keeps across restarts
#[derive(Clone)]
struct Settings {
  /// The MODEL of the workspace's inference speech goes to
  model: Option<String>,
  submit: bool,
  key: String,
  /// Load the model at start, rather than at the first dictation
  preload: bool,
  /// Minutes the loaded model stays in GPU memory while idle; None keeps
  /// it there always
  warm: Option<u64>,
}

struct State {
  settings: Settings,
  recording: Option<audio::Recording>,
  /// The human holds the key while the microphone opens
  opening: bool,
  /// A thread opens the microphone; a press meanwhile waits for it
  /// rather than opening another
  opener: bool,
  transcribing: bool,
  /// The model is loading, at start or for a dictation
  loading: bool,
  /// The last transcription, in memory only, for the human to copy
  last: Option<String>,
  error: Option<String>,
  /// Counts changes, so a watch sees each
  version: u64,
}

struct Seat {
  state: Mutex<State>,
  changed: Condvar,
}

static SEAT: OnceLock<Seat> = OnceLock::new();

fn settings_path() -> PathBuf {
  std::env::var_os("XDG_STATE_HOME")
    .map(PathBuf::from)
    .unwrap_or_else(|| {
      PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
        .join(".local/state")
    })
    .join("hunch/seat.json")
}

fn load() -> Settings {
  let saved: Value = std::fs::read_to_string(settings_path())
    .ok()
    .and_then(|s| serde_json::from_str(&s).ok())
    .unwrap_or_default();
  Settings {
    model: saved["model"].as_str().map(String::from),
    submit: saved["submit"].as_bool().unwrap_or(true),
    preload: saved["preload"].as_bool().unwrap_or(true),
    warm: saved["warm"].as_u64(),
    key: saved["key"]
      .as_str()
      .filter(|k| dictation_key(k).is_some())
      .unwrap_or(DEFAULT_KEY)
      .into(),
  }
}

fn save(settings: &Settings) {
  let path = settings_path();
  let json = json!({
      "model": settings.model,
      "submit": settings.submit,
      "key": settings.key,
      "preload": settings.preload,
      "warm": settings.warm,
  });
  let saved = path
    .parent()
    .map_or(Ok(()), std::fs::create_dir_all)
    .and_then(|()| std::fs::write(&path, json.to_string()));
  if let Err(e) = saved {
    eprintln!("hunch-seat: can't save the seat to {}: {e}", path.display());
  }
}

fn seat() -> &'static Seat {
  SEAT.get_or_init(|| Seat {
    state: Mutex::new(State {
      settings: load(),
      recording: None,
      opening: false,
      opener: false,
      transcribing: false,
      loading: false,
      last: None,
      error: None,
      version: 0,
    }),
    changed: Condvar::new(),
  })
}

fn lock() -> MutexGuard<'static, State> {
  seat().state.lock().unwrap_or_else(|e| e.into_inner())
}

/// Applies a change and wakes every watch
fn change(apply: impl FnOnce(&mut State)) {
  let mut state = lock();
  apply(&mut state);
  state.version += 1;
  seat().changed.notify_all();
}

fn context(state: &State) -> Value {
  json!({
      "idle": platform::idle(),
      "locked": platform::locked(),
      "wm": WM,
      "recording": state.recording.is_some(),
      "opening": state.opening,
      "transcribing": state.transcribing,
      "loading": state.loading,
      "model": state.settings.model,
      "submit": state.settings.submit,
      "key": state.settings.key,
      "preload": state.settings.preload,
      "warm": state.settings.warm,
      "error": state.error,
  })
}

/// Loads the model, if it isn't, telling watches while it does
#[cfg(feature = "inference")]
fn load_model(model: &str) -> Result<()> {
  if inference::loaded_id().as_deref() == Some(model) {
    return Ok(());
  }
  change(|s| s.loading = true);
  let loaded = inference::load(model);
  change(|s| s.loading = false);
  loaded
}

/// Loads the model in the background, so the first dictation doesn't wait
fn preload(model: String) {
  thread::spawn(move || {
    if let Err(e) = load_here(&model) {
      change(|s| s.error = Some(e.to_string()));
    }
  });
}

/// Loads the model if this seat runs it; a server loads its models itself
fn load_here(model: &str) -> Result<()> {
  #[cfg(feature = "inference")]
  if inference::find(model).is_ok() {
    return load_model(model);
  }
  let _ = model;
  Ok(())
}

/// Runs hunch-9p ARGS on the host's workspace, INPUT on its stdin; its
/// stdout
fn tree(args: &[&str], input: &[u8]) -> Result<Vec<u8>> {
  let mut child = Command::new(hunch::host::sibling("hunch-9p"))
    .args(args)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .map_err(|e| failed(format!("can't run hunch-9p: {e}")))?;
  let mut stdin = child.stdin.take().expect("piped");
  // On a thread, so a big input and a big answer don't wait for each other
  let input = input.to_vec();
  let writer = thread::spawn(move || stdin.write_all(&input));
  let output = child
    .wait_with_output()
    .map_err(|e| failed(format!("hunch-9p didn't finish: {e}")))?;
  let _ = writer.join();
  if !output.status.success() {
    return Err(failed(
      String::from_utf8_lossy(&output.stderr).trim().to_string(),
    ));
  }
  Ok(output.stdout)
}

fn read_json(path: &str) -> Result<Value> {
  let out = tree(&["read", path], &[])?;
  serde_json::from_slice(&out)
    .map_err(|e| failed(format!("{path} isn't JSON: {e}")))
}

/// MODEL, as the workspace's inference/models lists it
fn find_model(model: &str) -> Result<Value> {
  let models = read_json("inference/models")?;
  let models = models.as_array().into_iter().flatten();
  models
    .into_iter()
    .find(|m| m["id"] == model)
    .cloned()
    .ok_or_else(|| Error::Unknown(format!("inference has no model {model}")))
}

/// The text of pcm, by MODEL, through the workspace's inference/transcribe
fn transcribe(model: &str, pcm: &[f32]) -> Result<String> {
  let inputs = find_model(model)?["inputs"].clone();
  let (kind, speech) = encode(&inputs, pcm)?;
  let request = json!({"model": model, "type": kind});
  let input = [format!("{request}\n").as_bytes(), &speech].concat();
  let text = tree(&["rdwr", "inference/transcribe"], &input)?;
  Ok(String::from_utf8_lossy(&text).trim().to_string())
}

/// Starts watching the dictation key; the service calls it once, before
/// it starts any thread
pub fn start() {
  let settings = lock().settings.clone();
  // ggml reads it once, when it loads the first model, and lets the
  // model's GPU memory go after it's idle this long
  #[cfg(feature = "inference")]
  {
    let seconds = settings
      .warm
      .map_or(WARM_ALWAYS.to_string(), |m| (m * 60).to_string());
    // SAFETY: no other thread runs yet to read the environment
    unsafe { std::env::set_var(WARM_VAR, seconds) };
  }
  if let (true, Some(model)) = (settings.preload, settings.model.clone()) {
    preload(model);
  }
  // Opening the microphone can take a while, longer if macOS asks the
  // human first, so keys are handled on a thread of their own
  let (keys, pressed) = mpsc::channel();
  thread::spawn(move || {
    for key in pressed {
      on_key(key);
    }
  });
  thread::spawn(move || {
    loop {
      // A tap made before the grant stays deaf after it
      if !platform::may_listen_to_keys() {
        change(|s| s.error = Some(platform::KEYS_NEED.into()));
        thread::sleep(KEYS_RETRY);
        continue;
      }
      change(|s| s.error = None);
      let keys = keys.clone();
      if let Err(e) = platform::watch_keys(move |key| {
        let _ = keys.send(key);
      }) {
        change(|s| s.error = Some(e));
        thread::sleep(KEYS_RETRY);
      }
    }
  });
}

fn dictation_key(name: &str) -> Option<&'static platform::DictationKey> {
  platform::DICTATION_KEYS.iter().find(|k| k.name == name)
}

/// For needs: macOS's Accessibility needs asks on its own
#[cfg(target_os = "linux")]
pub fn may_listen_to_keys() -> bool {
  platform::may_listen_to_keys()
}

fn on_key(key: Key) {
  let name = lock().settings.key.clone();
  let dictation = dictation_key(&name).expect("set checks the key");
  match (dictation.down(&key), key) {
    (Some(true), _) => press(),
    (Some(false), _) => release(),
    (None, Key::Escape) => cancel(),
    // Another key may make a shortcut of the dictation key, or come
    // from a program rather than the human: either way the words
    // spoken so far are kept, though not sent
    (None, Key::Modifier { from, .. } | Key::Other { from }) => interrupt(from),
  }
}

fn interrupt(from: i64) {
  if lock().opening {
    return change(|s| s.opening = false);
  }
  if lock().recording.is_some() {
    // Which program it was, if not the keyboard; never which key
    eprintln!("hunch-seat: a key from pid {from} stops the dictation");
    finish(false);
  }
}

fn press() {
  if lock().recording.is_some() || lock().opening {
    return;
  }
  if lock().settings.model.clone().is_none() {
    return change(|s| s.error = Some(NO_MODEL.into()));
  }
  eprintln!("hunch-seat: dictation starts");
  let mut open = false;
  change(|s| {
    s.opening = true;
    s.error = None;
    open = !s.opener;
    s.opener = true;
  });
  // A microphone waking from sleep can take seconds to open: off the
  // key thread, so a release meanwhile doesn't wait in line behind it
  if open {
    thread::spawn(open_microphone);
  }
}

fn open_microphone() {
  let opening = Instant::now();
  let recording = audio::Recording::start();
  let took = opening.elapsed();
  eprintln!(
    "hunch-seat: the microphone took {} ms to open",
    took.as_millis()
  );
  change(|s| {
    s.opener = false;
    let held = std::mem::take(&mut s.opening);
    match recording {
      Ok(recording) if held => s.recording = Some(recording),
      Ok(_) => {
        s.error = Some(format!(
          "the microphone took {:.1} s to open: hold the key until \
           Listening…",
          took.as_secs_f64()
        ))
      }
      Err(e) => s.error = Some(e),
    }
  });
}

fn cancel() {
  if lock().recording.is_some() || lock().opening {
    change(|s| {
      s.recording = None;
      s.opening = false;
    });
  }
}

fn release() {
  if lock().opening {
    return change(|s| s.opening = false);
  }
  if lock().recording.is_some() {
    eprintln!("hunch-seat: dictation ends, transcribing");
    finish(true);
  }
}

/// Transcribes the recording, and sends the text if send
fn finish(send: bool) {
  let Some(recording) = lock().recording.take() else {
    return;
  };
  let settings = lock().settings.clone();
  change(|s| s.transcribing = true);
  thread::spawn(move || {
    let result = dictate(recording, &settings, send);
    if let Err(e) = &result {
      eprintln!("hunch-seat: dictation failed: {e}");
    }
    change(|s| {
      s.transcribing = false;
      s.error = result.err().map(|e| e.to_string());
    });
  });
}

fn dictate(
  recording: audio::Recording,
  settings: &Settings,
  send: bool,
) -> Result<()> {
  let pcm = recording.stop();
  if pcm.is_empty() {
    return Err(failed(
      "the key went up before the microphone sent any sound",
    ));
  }
  if pcm.iter().all(|&s| s == 0.0) {
    return Err(failed(format!(
      "the microphone sent nothing but silence: {}",
      platform::MIC_SILENT
    )));
  }
  let model = settings.model.clone().ok_or_else(|| failed(NO_MODEL))?;
  let text = transcribe(&model, &pcm)?;
  if text.is_empty() {
    eprintln!("hunch-seat: dictation heard no words");
    return Ok(());
  }
  // Kept even if it doesn't arrive: that's when the human needs it
  change(|s| s.last = Some(text.clone()));
  if !send {
    return Err(failed(
      "another key stopped the dictation; the text waits in Copy Last \
       Transcription",
    ));
  }
  // How much, never what: the log isn't for the human's words
  eprintln!(
    "hunch-seat: dictation sends {} characters",
    text.chars().count()
  );
  // Never to another window instead: text that didn't arrive fails, and
  // waits for the human to copy it
  deliver(&text, settings.submit).map_err(|e| {
    failed(format!("{e}; the text waits in Copy Last Transcription"))
  })
}

/// Types TEXT into the window in focus
fn deliver(text: &str, submit: bool) -> Result<()> {
  let wm = wm::open(WM)?;
  let focused = wm.context(None)?.window;
  let window = focused.ok_or_else(|| failed("no window has the focus"))?.id;
  wm.send(&window, text)?;
  if submit {
    wm.submit(&window)?;
  }
  Ok(())
}

/// PCM as the first of INPUTS this seat writes
fn encode(inputs: &Value, pcm: &[f32]) -> Result<(&'static str, Vec<u8>)> {
  let inputs = inputs.as_array().into_iter().flatten();
  for input in inputs.filter_map(Value::as_str) {
    match input {
      opus::TYPE => return Ok((opus::TYPE, opus::write(pcm)?)),
      wav::TYPE => return Ok((wav::TYPE, wav::write(pcm))),
      _ => {}
    }
  }
  Err(failed(format!(
    "the model takes none of {}, {}, which the seat writes",
    opus::TYPE,
    wav::TYPE
  )))
}

/// Frees the memory of this seat's model; a server frees its own
fn eject() -> Result<()> {
  #[cfg(feature = "inference")]
  inference::unload();
  // A watch shows whether it's loaded
  change(|_| {});
  Ok(())
}

fn watch(out: &mut File, gone: &AtomicBool) -> Result<()> {
  let mut seen = None;
  let mut state = lock();
  while !gone.load(Ordering::Relaxed) {
    if seen != Some(state.version) {
      seen = Some(state.version);
      // A client that's gone ends the watch
      if writeln!(out, "{}", context(&state)).is_err() {
        break;
      }
    }
    state = seat()
      .changed
      .wait_timeout(state, WATCH_CHECK)
      .unwrap_or_else(|e| e.into_inner())
      .0;
  }
  Ok(())
}

fn set(apply: impl FnOnce(&mut Settings)) {
  change(|s| apply(&mut s.settings));
  save(&lock().settings);
}

/// Answers a seat command in the service, writing to the client's stdout
pub fn run(
  args: &[String],
  _input: &mut File,
  out: &mut File,
  gone: &AtomicBool,
) -> Result<()> {
  let args: Vec<&str> = args.iter().map(String::as_str).collect();
  let said = match args.as_slice() {
    ["-h" | "--help", ..] => USAGE.to_string(),
    ["context"] => context(&lock()).to_string(),
    ["last"] => lock()
      .last
      .clone()
      .ok_or_else(|| failed("nothing dictated since the service started"))?,
    ["watch"] => return watch(out, gone),
    // For the tray, which reads the seat's files
    ["needs"] => crate::listed(false, true).to_string(),
    ["load"] => {
      let settings = lock().settings.clone();
      let model = settings.model.clone().ok_or_else(|| failed(NO_MODEL))?;
      load_here(&model)?;
      format!("loaded {model}")
    }
    ["eject"] => {
      eject()?;
      "ejected".into()
    }
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
    ["model", model] => {
      // Not under the lock: a server answers over the network
      let found = find_model(model)?;
      let takes = |field: &str, prefix: &str| {
        let types = found[field].as_array().into_iter().flatten();
        types
          .into_iter()
          .filter_map(Value::as_str)
          .any(|t| t.starts_with(prefix))
      };
      if !(takes("inputs", "audio/") && takes("outputs", "text/")) {
        return Err(failed(format!("{model} doesn't turn speech into text")));
      }
      set(|s| s.model = Some(model.to_string()));
      if lock().settings.preload {
        preload(model.to_string());
      }
      format!("model {model}")
    }
    ["key", key] => {
      if dictation_key(key).is_none() {
        let keys: Vec<&str> =
          platform::DICTATION_KEYS.iter().map(|k| k.name).collect();
        return Err(failed(format!(
          "KEY is one of {}, not {key}",
          keys.join(", ")
        )));
      }
      set(|s| s.key = key.to_string());
      format!("key {key}")
    }
    ["warm", "always"] => {
      set(|s| s.warm = None);
      "warm always, from the service's next start".into()
    }
    ["warm", minutes] => {
      let minutes: u64 =
        minutes.parse().ok().filter(|m| *m > 0).ok_or_else(|| {
          failed(format!(
            "MINUTES is a whole number above 0, or always, not {minutes}"
          ))
        })?;
      set(|s| s.warm = Some(minutes));
      format!("warm {minutes}, from the service's next start")
    }
    ["preload", on @ ("on" | "off")] => {
      let preloading = *on == "on";
      set(|s| s.preload = preloading);
      let settings = lock().settings.clone();
      match settings.model.clone() {
        Some(model) if preloading => preload(model),
        // Off frees the memory now, not only after a restart
        #[cfg(feature = "inference")]
        _ if !preloading => inference::unload(),
        _ => {}
      }
      format!("preload {on}")
    }
    ["submit", on @ ("on" | "off")] => {
      set(|s| s.submit = *on == "on");
      format!("submit {on}")
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
