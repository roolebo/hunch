//! The seat's keyboards and pointers on Linux, read from evdev: as on a
//! text console, so under any desktop, X11 or Wayland. Reading them takes
//! the group input, which lets the human's programs read every key, so
//! the seat asks for it rather than takes it.

use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const EVENTS: &str = "/dev/input";
const EV_KEY: u16 = 1;
const KEY_ESC: u16 = 1;
/// Codes from here on are buttons, a pointer's or a pad's, not keys
const BTN_MISC: u16 = 0x100;
const RELEASED: i32 = 0;
const PRESSED: i32 = 1;
/// How often the seat looks for keyboards plugged in since
const RESCAN: Duration = Duration::from_secs(5);

pub const KEYS_NEED: &str = "the dictation key needs group input: \
  usermod -aG input $USER, then log in again";

/// Why a microphone may send only silence
pub const MIC_SILENT: &str = "check it isn't muted, in Settings, Sound";

/// A key the dictation listens for
pub enum Key {
  /// A key that can dictate went down or up; from is always 0, the
  /// keyboard, since evdev doesn't tell which program posted a key
  Modifier {
    code: u16,
    down: bool,
    from: i64,
  },
  Escape,
  Other {
    from: i64,
  },
}

/// A key that dictates while held
pub struct DictationKey {
  pub name: &'static str,
  code: u16,
}

/// Linux's input-event-codes.h; fn is the keyboard's own, never sent
pub const DICTATION_KEYS: &[DictationKey] = &[
  DictationKey {
    name: "right-super",
    code: 126,
  },
  DictationKey {
    name: "right-alt",
    code: 100,
  },
  DictationKey {
    name: "right-control",
    code: 97,
  },
  DictationKey {
    name: "right-shift",
    code: 54,
  },
];

impl DictationKey {
  /// Whether a key change pressed or released this key, if it's this one
  pub fn down(&self, key: &Key) -> Option<bool> {
    match key {
      Key::Modifier { code, down, .. } if *code == self.code => Some(*down),
      _ => None,
    }
  }
}

/// struct input_event, as the kernel writes it on a 64-bit host
#[repr(C)]
struct InputEvent {
  time: libc::timeval,
  kind: u16,
  code: u16,
  value: i32,
}

/// When the human last used a keyboard or pointer
static LAST_INPUT: Mutex<Option<Instant>> = Mutex::new(None);
static STARTED: Mutex<Option<Instant>> = Mutex::new(None);

fn devices() -> Vec<PathBuf> {
  let Ok(entries) = std::fs::read_dir(EVENTS) else {
    return vec![];
  };
  entries
    .flatten()
    .map(|e| e.path())
    .filter(|p| {
      p.file_name()
        .is_some_and(|n| n.to_string_lossy().starts_with("event"))
    })
    .collect()
}

pub fn may_listen_to_keys() -> bool {
  devices().iter().any(|d| File::open(d).is_ok())
}

/// Seconds since the human last used a keyboard or pointer; since the
/// seat started watching, before any
pub fn idle() -> f64 {
  let since = LAST_INPUT
    .lock()
    .unwrap_or_else(|e| e.into_inner())
    .or(*STARTED.lock().unwrap_or_else(|e| e.into_inner()));
  since.map_or(0.0, |t| t.elapsed().as_secs_f64())
}

/// Not known on Linux yet; the Shell extension refuses to type while the
/// screen is locked
pub fn locked() -> bool {
  false
}

fn key(event: &InputEvent) -> Option<Key> {
  if event.kind != EV_KEY || event.code >= BTN_MISC {
    return None;
  }
  let dictates = DICTATION_KEYS.iter().any(|k| k.code == event.code);
  match (event.code, event.value) {
    (KEY_ESC, PRESSED) => Some(Key::Escape),
    (code, PRESSED | RELEASED) if dictates => Some(Key::Modifier {
      code,
      down: event.value == PRESSED,
      from: 0,
    }),
    (_, PRESSED) => Some(Key::Other { from: 0 }),
    // A key held repeats, and a key let go is no interruption
    _ => None,
  }
}

type On = Arc<Mutex<Box<dyn FnMut(Key) + Send>>>;

/// Reads one device until it's unplugged
fn read(mut device: File, on: On) {
  let mut buffer = [0u8; size_of::<InputEvent>()];
  while device.read_exact(&mut buffer).is_ok() {
    // SAFETY: the kernel writes whole input_events, and any bytes make one
    let event: InputEvent =
      unsafe { std::ptr::read_unaligned(buffer.as_ptr().cast()) };
    *LAST_INPUT.lock().unwrap_or_else(|e| e.into_inner()) =
      Some(Instant::now());
    if let Some(key) = key(&event) {
      (on.lock().unwrap_or_else(|e| e.into_inner()))(key);
    }
  }
}

/// Calls on for each key the dictation listens for, from every keyboard,
/// those plugged in later too; fails if it can read none
pub fn watch_keys(on: impl FnMut(Key) + Send + 'static) -> Result<(), String> {
  STARTED
    .lock()
    .unwrap_or_else(|e| e.into_inner())
    .get_or_insert_with(Instant::now);
  let on: On = Arc::new(Mutex::new(Box::new(on)));
  let watched: Arc<Mutex<HashSet<PathBuf>>> = Arc::default();
  loop {
    for path in devices() {
      if watched
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(&path)
      {
        continue;
      }
      let Ok(device) = File::open(&path) else {
        continue;
      };
      watched
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(path.clone());
      let (on, watched) = (Arc::clone(&on), Arc::clone(&watched));
      thread::spawn(move || {
        read(device, on);
        // Unplugged: the same path may come back as another device
        watched
          .lock()
          .unwrap_or_else(|e| e.into_inner())
          .remove(&path);
      });
    }
    if watched.lock().unwrap_or_else(|e| e.into_inner()).is_empty() {
      return Err(KEYS_NEED.into());
    }
    thread::sleep(RESCAN);
  }
}
