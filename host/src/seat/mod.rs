//! The seat role, as skills/hunch/SEAT.md specifies it, on macOS so far.
//! It lives in the service, which holds the dictation key, the recording
//! and the loaded model, since they outlive any one command.

mod audio;

#[cfg(feature = "compute")]
use crate::compute;
use crate::error::{Error, Result, failed};
use crate::macos::{self, Key};
use crate::{config, wav, wm};
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
Usage: hunch-host seat COMMAND [ARGUMENTS]

Commands:
  context               The seat's state, as JSON
  last                  The last transcription, kept in memory only
  watch                 The state again each time it changes, a line each
  model MODEL           Transcribe with MODEL
  target TARGET         Send dictation to TARGET
  submit on|off         Press Enter after the text, or not
  preload on|off        Load the model at start, or at the first dictation
  warm MINUTES|always   Keep the model in GPU memory this long while idle,
                        from the service's next start
  key KEY               Dictate while KEY is held: right-super,
                        right-alt, right-control, right-shift or fn
  compute COMPUTE       Transcribe with COMPUTE: localhost local:, or
                        HOST ADDR, a host in the microphone's allow

TARGET is focus, or HOST ADDR WINDOW. Escape cancels a dictation.";

const WM: &str = "macos:";
const FOCUS: &str = "focus";
const LOCALHOST: &str = "localhost";
const LOCAL: &str = "local:";
const COMPUTE: &str = "localhost local:";
#[cfg(feature = "compute")]
const WARM_VAR: &str = "GGML_METAL_RESIDENCY_KEEP_ALIVE_S";
/// Ten years, in seconds: as good as always, and within an int
#[cfg(feature = "compute")]
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
    /// COMPUTE, as HOST ADDR
    compute: String,
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
        compute: saved["compute"].as_str().unwrap_or(COMPUTE).into(),
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
        "compute": settings.compute,
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
        "idle": macos::idle(),
        "locked": macos::locked(),
        "wm": WM,
        "recording": state.recording.is_some(),
        "opening": state.opening,
        "transcribing": state.transcribing,
        "loading": state.loading,
        "compute": state.settings.compute,
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
#[cfg(feature = "compute")]
fn load_model(model: &str) -> Result<()> {
    if compute::loaded_id().as_deref() == Some(model) {
        return Ok(());
    }
    change(|s| s.loading = true);
    let loaded = compute::load(model);
    change(|s| s.loading = false);
    loaded
}

/// COMPUTE's host, and its ADDR
fn split(compute: &str) -> (&str, &str) {
    compute.split_once(' ').expect("compute is HOST ADDR")
}

/// Loads the model in the background, so the first dictation doesn't wait
fn preload(model: String) {
    let compute = lock().settings.compute.clone();
    thread::spawn(move || {
        if let Err(e) = load_on(&compute, &model) {
            change(|s| s.error = Some(e.to_string()));
        }
    });
}

/// Has COMPUTE load the model
fn load_on(compute: &str, model: &str) -> Result<()> {
    match split(compute) {
        #[cfg(feature = "compute")]
        (LOCALHOST, LOCAL) => load_model(model),
        (LOCALHOST, addr) => Err(no_compute(addr)),
        (host, addr) => {
            change(|s| s.loading = true);
            let loaded = ssh(host, &["compute", addr, "load", model], &[]);
            change(|s| s.loading = false);
            loaded.map(drop)
        }
    }
}

/// Fails unless COMPUTE runs the model
fn known(compute: &str, model: &str) -> Result<()> {
    match split(compute) {
        #[cfg(feature = "compute")]
        (LOCALHOST, LOCAL) => compute::find(model).map(drop),
        (LOCALHOST, addr) => Err(no_compute(addr)),
        (host, addr) => {
            let models = ssh(host, &["compute", addr, "models"], &[])?;
            let models: Value = serde_json::from_slice(&models)
                .map_err(|e| failed(format!("{host}'s models aren't JSON: {e}")))?;
            let known = models
                .as_array()
                .into_iter()
                .flatten()
                .any(|m| m["id"] == model);
            known
                .then_some(())
                .ok_or_else(|| Error::Unknown(format!("{host} has no model {model}")))
        }
    }
}

fn no_compute(addr: &str) -> Error {
    failed(format!(
        "this seat runs no compute {addr}: pick one on another host"
    ))
}

/// Fails unless the microphone's allow lists host, as SEAT.md says
fn allowed(host: &str) -> Result<()> {
    let mic = audio::mic().map_err(failed)?;
    if config::device_allow(&mic).iter().any(|h| h == host) {
        return Ok(());
    }
    Err(failed(format!(
        "{host} isn't in the allow of the microphone {mic}; the human adds it in the seat's config"
    )))
}

/// The text of pcm, by COMPUTE
fn transcribe(compute: &str, model: &str, pcm: &[f32]) -> Result<String> {
    match split(compute) {
        #[cfg(feature = "compute")]
        (LOCALHOST, LOCAL) => {
            load_model(model)?;
            compute::transcribe(model, pcm)
        }
        (LOCALHOST, addr) => Err(no_compute(addr)),
        (host, addr) => {
            // Checked again, since the human may have changed it since
            allowed(host)?;
            let text = ssh(
                host,
                &["compute", addr, "run", model, wav::TYPE],
                &wav::write(pcm),
            )?;
            Ok(String::from_utf8_lossy(&text).trim_end().to_string())
        }
    }
}

/// Starts watching the dictation key; the service calls it once, before
/// it starts any thread
pub fn start() {
    let settings = lock().settings.clone();
    // ggml reads it once, when it loads the first model, and lets the
    // model's GPU memory go after it's idle this long
    #[cfg(feature = "compute")]
    {
        let seconds = settings
            .warm
            .map_or(WARM_ALWAYS.to_string(), |m| (m * 60).to_string());
        // SAFETY: no other thread runs yet to read the environment
        unsafe { std::env::set_var(WARM_VAR, seconds) };
    }
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
                change(|s| s.error = Some("the dictation key needs Accessibility".into()));
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
        eprintln!("hunch-host: a key from pid {from} stops the dictation");
        finish(false);
    }
}

fn press() {
    if lock().recording.is_some() || lock().opening {
        return;
    }
    if lock().settings.model.is_none() {
        return change(|s| s.error = Some("no model: pick one in the tray".into()));
    }
    eprintln!("hunch-host: dictation starts");
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
        "hunch-host: the microphone took {} ms to open",
        took.as_millis()
    );
    change(|s| {
        s.opener = false;
        let held = std::mem::take(&mut s.opening);
        match recording {
            Ok(recording) if held => s.recording = Some(recording),
            Ok(_) => {
                s.error = Some(format!(
                    "the microphone took {:.1} s to open: hold the key until Listening…",
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
        eprintln!("hunch-host: dictation ends, transcribing");
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
            eprintln!("hunch-host: dictation failed: {e}");
        }
        change(|s| {
            s.transcribing = false;
            s.error = result.err().map(|e| e.to_string());
        });
    });
}

fn dictate(recording: audio::Recording, settings: &Settings, send: bool) -> Result<()> {
    let pcm = recording.stop();
    if pcm.is_empty() {
        return Err(failed(
            "the key went up before the microphone sent any sound",
        ));
    }
    if pcm.iter().all(|&s| s == 0.0) {
        return Err(failed(
            "the microphone sent nothing but silence: check Microphone in System Settings",
        ));
    }
    let model = settings.model.as_deref().expect("press checks for a model");
    let text = transcribe(&settings.compute, model, &pcm)?;
    if text.is_empty() {
        eprintln!("hunch-host: dictation heard no words");
        return Ok(());
    }
    // Kept even if it doesn't arrive: that's when the human needs it
    change(|s| s.last = Some(text.clone()));
    if !send {
        return Err(failed(
            "another key stopped the dictation; the text waits in Copy Last Transcription",
        ));
    }
    // How much and where, never what: the log isn't for the human's words
    eprintln!(
        "hunch-host: dictation sends {} characters to {}",
        text.chars().count(),
        settings.target
    );
    // Never to another window instead: text that didn't arrive where the
    // human meant it to fails, and waits for them to copy it
    let delivered = deliver(&settings.target, &text, settings.submit);
    if let Err(Error::Gone(_)) = delivered {
        // The window picked is closed: the next dictation goes to the focus
        change(|s| s.settings.target = FOCUS.into());
        save(&lock().settings);
    }
    delivered.map_err(|e| failed(format!("{e}; the text waits in Copy Last Transcription")))
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
            ssh(host, &["wm", addr, "send", window], text.as_bytes())?;
            if submit {
                ssh(host, &["wm", addr, "submit", window], &[])?;
            }
            Ok(())
        }
        _ => Err(failed(format!(
            "TARGET isn't focus or HOST ADDR WINDOW: {target}"
        ))),
    }
}

/// Runs hunch-host ARGS on host, input on its stdin; its stdout
fn ssh(host: &str, args: &[&str], input: &[u8]) -> Result<Vec<u8>> {
    let mut child = Command::new("ssh")
        // The service has no one to ask for a password or a host key
        .args(["-o", "BatchMode=yes", host, "hunch-host"])
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| failed(format!("can't run ssh: {e}")))?;
    let mut stdin = child.stdin.take().expect("piped");
    // On a thread, so a big input and a big answer don't wait for each other
    let input = input.to_vec();
    let writer = thread::spawn(move || stdin.write_all(&input));
    let output = child
        .wait_with_output()
        .map_err(|e| failed(format!("ssh {host} didn't finish: {e}")))?;
    let written = writer.join().expect("write doesn't panic");
    match output.status.code() {
        Some(0) => {
            written.map_err(|e| failed(format!("can't send to {host}: {e}")))?;
            Ok(output.stdout)
        }
        status => {
            let message = format!("{host}: {}", String::from_utf8_lossy(&output.stderr).trim());
            Err(match status {
                Some(4) => Error::Gone(message),
                _ => failed(message),
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
        ["last"] => lock()
            .last
            .clone()
            .ok_or_else(|| failed("nothing dictated since the service started"))?,
        ["watch"] => return watch(out, gone),
        ["model", model] => {
            // Not under the lock: a compute elsewhere answers over ssh
            let compute = lock().settings.compute.clone();
            known(&compute, model)?;
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
            let settings = lock().settings.clone();
            match settings.model {
                Some(model) if preloading => preload(model),
                // Off frees the memory now, not only after a restart
                #[cfg(feature = "compute")]
                _ if !preloading && settings.compute == COMPUTE => compute::unload(),
                _ => {}
            }
            format!("preload {on}")
        }
        ["submit", on @ ("on" | "off")] => {
            set(|s| s.submit = *on == "on");
            format!("submit {on}")
        }
        ["compute", host, addr] => {
            match (*host, *addr) {
                (LOCALHOST, LOCAL) if cfg!(feature = "compute") => {}
                (LOCALHOST, addr) => return Err(no_compute(addr)),
                (host, _) => allowed(host)?,
            }
            let compute = format!("{host} {addr}");
            set(|s| s.compute = compute.clone());
            format!("compute {compute}")
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
