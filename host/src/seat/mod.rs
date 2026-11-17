//! The seat role, as skills/hunch/SEAT.md specifies it, on macOS so far.
//! It lives in the service, which holds the dictation key, the recording
//! and the loaded model, since they outlive any one command.

mod audio;

use crate::error::{Error, Result, failed};
use crate::macos::{self, Key};
use crate::{compute, wm};
use serde_json::{Value, json};
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard, OnceLock, mpsc};
use std::thread;
use std::time::Duration;

pub const USAGE: &str = "\
Usage: hunch-host seat COMMAND [ARGUMENTS]

Commands:
  context               The seat's state, as JSON
  watch                 The state again each time it changes, a line each
  model MODEL           Transcribe with MODEL
  target TARGET         Send dictation to TARGET
  submit on|off         Press Enter after the text, or not
  preload on|off        Load the model at start, or at the first dictation
  warm MINUTES|always   Keep the model in GPU memory this long while idle,
                        from the service's next start
  key KEY               Dictate while KEY is held: right-command,
                        right-option, right-control, right-shift or fn
  compute COMPUTE       Transcribe with COMPUTE; localhost local: so far

TARGET is focus, or HOST ADDR WINDOW. Escape cancels a dictation.";

const WM: &str = "macos:";
const FOCUS: &str = "focus";
const LOCALHOST: &str = "localhost";
const COMPUTE: &str = "localhost local:";
const WARM_VAR: &str = "GGML_METAL_RESIDENCY_KEEP_ALIVE_S";
/// Ten years, in seconds: as good as always, and within an int
const WARM_ALWAYS: &str = "315360000";
/// Handy's default, which the human knows
const DEFAULT_KEY: &str = "right-command";
/// How often a watch checks whether its client is gone
const WATCH_CHECK: Duration = Duration::from_secs(1);
/// How long the seat waits for Input Monitoring before it watches keys
const KEYS_RETRY: Duration = Duration::from_secs(5);

/// What the seat keeps across restarts
#[derive(Clone)]
struct Settings {
    model: Option<String>,
    target: String,
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
    transcribing: bool,
    /// The model is loading, at start or for a dictation
    loading: bool,
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
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state")
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
        target: saved["target"].as_str().unwrap_or(FOCUS).into(),
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
        "target": settings.target,
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
        eprintln!("hunch-host: can't save the seat to {}: {e}", path.display());
    }
}

fn seat() -> &'static Seat {
    SEAT.get_or_init(|| Seat {
        state: Mutex::new(State {
            settings: load(),
            recording: None,
            transcribing: false,
            loading: false,
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
        "idle": macos::idle(),
        "locked": macos::locked(),
        "wm": WM,
        "recording": state.recording.is_some(),
        "transcribing": state.transcribing,
        "loading": state.loading,
        "compute": COMPUTE,
        "model": state.settings.model,
        "target": state.settings.target,
        "submit": state.settings.submit,
        "key": state.settings.key,
        "preload": state.settings.preload,
        "warm": state.settings.warm,
        "error": state.error,
    })
}

/// Loads the model, if it isn't, telling watches while it does
fn load_model(model: &str) -> Result<()> {
    if compute::loaded_id().as_deref() == Some(model) {
        return Ok(());
    }
    change(|s| s.loading = true);
    let loaded = compute::load(model);
    change(|s| s.loading = false);
    loaded
}

/// Loads the model in the background, so the first dictation doesn't wait
fn preload(model: String) {
    thread::spawn(move || {
        if let Err(e) = load_model(&model) {
            change(|s| s.error = Some(e.to_string()));
        }
    });
}

/// Starts watching the dictation key; the service calls it once, before
/// it starts any thread
pub fn start() {
    let settings = lock().settings.clone();
    // ggml reads it once, when it loads the first model, and lets the
    // model's GPU memory go after it's idle this long
    let seconds = settings
        .warm
        .map_or(WARM_ALWAYS.to_string(), |m| (m * 60).to_string());
    // SAFETY: no other thread runs yet to read the environment
    unsafe { std::env::set_var(WARM_VAR, seconds) };
    if let (true, Some(model)) = (settings.preload, settings.model) {
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
            if !macos::may_listen_to_keys() {
                change(|s| s.error = Some("the dictation key needs Input Monitoring".into()));
                thread::sleep(KEYS_RETRY);
                continue;
            }
            change(|s| s.error = None);
            let keys = keys.clone();
            if let Err(e) = macos::watch_keys(move |key| {
                let _ = keys.send(key);
            }) {
                change(|s| s.error = Some(e));
                thread::sleep(KEYS_RETRY);
            }
        }
    });
}

fn dictation_key(name: &str) -> Option<&'static macos::DictationKey> {
    macos::DICTATION_KEYS.iter().find(|k| k.name == name)
}

fn on_key(key: Key) {
    let name = lock().settings.key.clone();
    let dictation = dictation_key(&name).expect("set checks the key");
    match (dictation.down(&key), key) {
        (Some(true), _) => press(),
        (Some(false), _) => release(),
        // Another modifier goes with the key into a shortcut
        (None, Key::Modifier { .. }) => cancel(),
        // So does a key typed while it's held
        (None, Key::Escape | Key::Other) => cancel(),
    }
}

fn press() {
    if lock().recording.is_some() {
        return;
    }
    if lock().settings.model.is_none() {
        return change(|s| s.error = Some("no model: pick one in the tray".into()));
    }
    eprintln!("hunch-host: dictation starts");
    let recording = audio::Recording::start();
    change(|s| match recording {
        Ok(recording) => {
            s.recording = Some(recording);
            s.error = None;
        }
        Err(e) => s.error = Some(e),
    });
}

fn cancel() {
    if lock().recording.is_some() {
        change(|s| s.recording = None);
    }
}

fn release() {
    let Some(recording) = lock().recording.take() else {
        return;
    };
    let settings = lock().settings.clone();
    eprintln!("hunch-host: dictation ends, transcribing");
    change(|s| s.transcribing = true);
    thread::spawn(move || {
        let result = dictate(recording, &settings);
        if let Err(e) = &result {
            eprintln!("hunch-host: dictation failed: {e}");
        }
        change(|s| {
            s.transcribing = false;
            s.error = result.err().map(|e| e.to_string());
        });
    });
}

fn dictate(recording: audio::Recording, settings: &Settings) -> Result<()> {
    let pcm = recording.stop();
    if pcm.iter().all(|&s| s == 0.0) {
        return Err(failed(
            "the microphone sent nothing but silence: check Microphone in System Settings",
        ));
    }
    let model = settings.model.as_deref().expect("press checks for a model");
    load_model(model)?;
    let text = compute::transcribe(model, &pcm)?;
    if text.is_empty() {
        eprintln!("hunch-host: dictation heard no words");
        return Ok(());
    }
    // How much and where, never what: the log isn't for the human's words
    eprintln!(
        "hunch-host: dictation sends {} characters to {}",
        text.chars().count(),
        settings.target
    );
    match deliver(&settings.target, &text, settings.submit) {
        // The window picked is gone: back to the focus, as the human sees
        Err(Error::Gone(_)) => {
            change(|s| s.settings.target = FOCUS.into());
            save(&lock().settings);
            deliver(FOCUS, &text, settings.submit)
        }
        delivered => delivered,
    }
}

fn deliver(target: &str, text: &str, submit: bool) -> Result<()> {
    let parts: Vec<&str> = target.split(' ').collect();
    match parts.as_slice() {
        [FOCUS] => {
            let wm = wm::open(WM)?;
            let window = wm
                .context(None)?
                .window
                .ok_or_else(|| failed("no window has the focus"))?;
            wm.send(&window.id, text)?;
            if submit {
                wm.submit(&window.id)?;
            }
            Ok(())
        }
        [LOCALHOST, addr, window] => {
            let wm = wm::open(addr)?;
            wm.send(window, text)?;
            if submit {
                wm.submit(window)?;
            }
            Ok(())
        }
        [host, addr, window] => {
            ssh(host, &["wm", addr, "send", window], Some(text))?;
            if submit {
                ssh(host, &["wm", addr, "submit", window], None)?;
            }
            Ok(())
        }
        _ => Err(failed(format!(
            "TARGET isn't focus or HOST ADDR WINDOW: {target}"
        ))),
    }
}

fn ssh(host: &str, args: &[&str], input: Option<&str>) -> Result<()> {
    let mut child = Command::new("ssh")
        .arg(host)
        .arg("hunch-host")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| failed(format!("can't run ssh: {e}")))?;
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        stdin
            .write_all(input.as_bytes())
            .map_err(|e| failed(format!("can't send the text to {host}: {e}")))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|e| failed(format!("ssh {host} didn't finish: {e}")))?;
    match output.status.code() {
        Some(0) => Ok(()),
        status => {
            let message = format!("{host}: {}", String::from_utf8_lossy(&output.stderr).trim());
            Err(if status == Some(4) {
                Error::Gone(message)
            } else {
                failed(message)
            })
        }
    }
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
pub fn run(args: &[String], _input: &mut File, out: &mut File, gone: &AtomicBool) -> Result<()> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let said = match args.as_slice() {
        ["-h" | "--help", ..] => USAGE.to_string(),
        ["context"] => context(&lock()).to_string(),
        ["watch"] => return watch(out, gone),
        ["model", model] => {
            compute::find(model)?;
            set(|s| s.model = Some(model.to_string()));
            if lock().settings.preload {
                preload(model.to_string());
            }
            format!("model {model}")
        }
        ["target", FOCUS] => {
            set(|s| s.target = FOCUS.into());
            "target focus".into()
        }
        ["target", host, addr, window] => {
            let target = format!("{host} {addr} {window}");
            set(|s| s.target = target.clone());
            format!("target {target}")
        }
        ["key", key] => {
            if dictation_key(key).is_none() {
                let keys: Vec<&str> = macos::DICTATION_KEYS.iter().map(|k| k.name).collect();
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
            let minutes: u64 = minutes.parse().ok().filter(|m| *m > 0).ok_or_else(|| {
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
            match lock().settings.model.clone() {
                Some(model) if preloading => preload(model),
                // Off frees the memory now, not only after a restart
                _ if !preloading => compute::unload(),
                _ => {}
            }
            format!("preload {on}")
        }
        ["submit", on @ ("on" | "off")] => {
            set(|s| s.submit = *on == "on");
            format!("submit {on}")
        }
        ["compute", "localhost", "local:"] => format!("compute {COMPUTE}"),
        ["compute", ..] => {
            return Err(failed(format!(
                "this seat transcribes with {COMPUTE} only, so far"
            )));
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
