//! The CoreFoundation, CoreGraphics and Accessibility calls hunch-host
//! needs on macOS. Window and display positions are global: logical pixels
//! from the top left of the main display.

use std::ffi::{CString, c_char, c_long, c_ulong, c_void};
use std::time::Duration;

type CFTypeRef = *const c_void;
type CFIndex = c_long;

const UTF8: u32 = 0x0800_0100;
const NUMBER_I64: CFIndex = 4;
const NUMBER_F64: CFIndex = 13;
const WINDOW_LIST_ALL: u32 = 0;
const WINDOW_LIST_ON_SCREEN_ONLY: u32 = 1 << 0;
const WINDOW_LIST_EXCLUDE_DESKTOP: u32 = 1 << 4;
const MAX_DISPLAYS: usize = 16;

type AXError = i32;
const AX_SUCCESS: AXError = 0;
const AX_VALUE_CGPOINT: u32 = 1;
const AX_VALUE_CGSIZE: u32 = 2;
/// How long an app gets to answer each accessibility question
const AX_TIMEOUT: Duration = Duration::from_millis(400);

const KEY_RETURN: u16 = 36;
/// CGEventKeyboardSetUnicodeString takes at most this many UTF-16 units
const UNICODE_CHUNK: usize = 20;
/// Key down to key up of a posted key
const KEY_HOLD: Duration = Duration::from_millis(5);
/// How far an app's frame of a window may be from the window server's and
/// still be the same window
const FRAME_SLACK: f64 = 2.0;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Rect {
    pub origin: Point,
    pub size: Size,
}

impl Rect {
    fn about(&self, other: &Rect) -> bool {
        [
            (self.origin.x, other.origin.x),
            (self.origin.y, other.origin.y),
            (self.size.width, other.size.width),
            (self.size.height, other.size.height),
        ]
        .iter()
        .all(|(a, b)| (a - b).abs() <= FRAME_SLACK)
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.origin.x
            && p.y >= self.origin.y
            && p.x < self.origin.x + self.size.width
            && p.y < self.origin.y + self.size.height
    }
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: CFTypeRef);
    fn CFArrayGetCount(array: CFTypeRef) -> CFIndex;
    fn CFArrayGetValueAtIndex(array: CFTypeRef, index: CFIndex) -> CFTypeRef;
    fn CFDictionaryGetValue(dict: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
    fn CFStringCreateWithCString(alloc: CFTypeRef, s: *const c_char, encoding: u32) -> CFTypeRef;
    fn CFStringGetLength(s: CFTypeRef) -> CFIndex;
    fn CFStringGetMaximumSizeForEncoding(length: CFIndex, encoding: u32) -> CFIndex;
    fn CFStringGetCString(s: CFTypeRef, buf: *mut c_char, size: CFIndex, encoding: u32) -> u8;
    fn CFNumberGetValue(number: CFTypeRef, kind: CFIndex, out: *mut c_void) -> u8;
    fn CFBooleanGetValue(boolean: CFTypeRef) -> u8;
    fn CFGetTypeID(cf: CFTypeRef) -> c_ulong;
    fn CFArrayGetTypeID() -> c_ulong;
    fn CFEqual(a: CFTypeRef, b: CFTypeRef) -> u8;
    fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
    static kCFBooleanTrue: CFTypeRef;
    static kCFBooleanFalse: CFTypeRef;
    static kCFRunLoopCommonModes: CFTypeRef;
    fn CFMachPortCreateRunLoopSource(
        alloc: CFTypeRef,
        port: CFTypeRef,
        order: CFIndex,
    ) -> CFTypeRef;
    fn CFRunLoopGetCurrent() -> CFTypeRef;
    fn CFRunLoopAddSource(run_loop: CFTypeRef, source: CFTypeRef, mode: CFTypeRef);
    fn CFRunLoopRun();
    fn CFRunLoopStop(run_loop: CFTypeRef);
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> CFTypeRef;
    fn AXUIElementCopyAttributeValue(
        element: CFTypeRef,
        name: CFTypeRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    fn AXUIElementSetAttributeValue(
        element: CFTypeRef,
        name: CFTypeRef,
        value: CFTypeRef,
    ) -> AXError;
    fn AXUIElementSetMessagingTimeout(element: CFTypeRef, seconds: f32) -> AXError;
    fn AXValueGetValue(value: CFTypeRef, kind: u32, out: *mut c_void) -> u8;
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> CFTypeRef;
    fn CGRectMakeWithDictionaryRepresentation(dict: CFTypeRef, rect: *mut Rect) -> u8;
    fn CGGetActiveDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
    fn CGDisplayBounds(display: u32) -> Rect;
    fn CGMainDisplayID() -> u32;
    fn CGEventCreate(source: CFTypeRef) -> CFTypeRef;
    fn CGEventGetLocation(event: CFTypeRef) -> Point;
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGPreflightPostEventAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
    fn CGRequestPostEventAccess() -> bool;
    fn CGPreflightListenEventAccess() -> bool;
    fn CGRequestListenEventAccess() -> bool;
    fn CGEventCreateKeyboardEvent(source: CFTypeRef, key: u16, down: bool) -> CFTypeRef;
    fn CGEventKeyboardSetUnicodeString(event: CFTypeRef, length: c_ulong, text: *const u16);
    fn CGEventPostToPid(pid: i32, event: CFTypeRef);
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        mask: u64,
        callback: TapCallback,
        info: *mut c_void,
    ) -> CFTypeRef;
    fn CGEventTapEnable(tap: CFTypeRef, enable: bool);
    fn CGEventGetIntegerValueField(event: CFTypeRef, field: u32) -> i64;
    fn CGEventGetFlags(event: CFTypeRef) -> u64;
    fn CGEventSourceSecondsSinceLastEventType(state: u32, kind: u32) -> f64;
    fn CGSessionCopyCurrentDictionary() -> CFTypeRef;
}

#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {
    static AVMediaTypeAudio: CFTypeRef;
}

#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> *const c_void;
    fn sel_registerName(name: *const c_char) -> *const c_void;
    fn objc_msgSend();
}

type TapCallback = extern "C" fn(*mut c_void, u32, CFTypeRef, *mut c_void) -> CFTypeRef;
const SESSION_TAP: u32 = 1;
const HEAD_INSERT: u32 = 0;
/// A tap that may swallow events, not only listen
const TAP_ACTIVE: u32 = 0;
/// A tap that only listens, so it can never hold up the human's input
const TAP_LISTEN: u32 = 1;
const LEFT_MOUSE_DOWN: u32 = 1;
const LEFT_MOUSE_UP: u32 = 2;
const KEY_DOWN: u32 = 10;
const TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
const KEYCODE_FIELD: u32 = 9;
const KEY_ESCAPE: i64 = 53;
const FLAGS_CHANGED: u32 = 12;
const COMBINED_SESSION_STATE: u32 = 0;
const ANY_INPUT_EVENT: u32 = u32::MAX;
/// AVAuthorizationStatusAuthorized
const AV_AUTHORIZED: isize = 3;

/// A CoreFoundation object this code created, released when dropped
struct Owned(CFTypeRef);

impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) }
    }
}

fn cf_string(s: &str) -> Owned {
    let c = CString::new(s).expect("keys have no NUL");
    Owned(unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), UTF8) })
}

/// A window's dictionary from CGWindowListCopyWindowInfo
struct Dict(CFTypeRef);

impl Dict {
    fn get(&self, key: &str) -> Option<CFTypeRef> {
        let key = cf_string(key);
        let value = unsafe { CFDictionaryGetValue(self.0, key.0) };
        (!value.is_null()).then_some(value)
    }

    fn string(&self, key: &str) -> Option<String> {
        let s = self.get(key)?;
        let size = unsafe { CFStringGetMaximumSizeForEncoding(CFStringGetLength(s), UTF8) } + 1;
        let mut buf = vec![0u8; size as usize];
        let ok = unsafe { CFStringGetCString(s, buf.as_mut_ptr().cast(), size, UTF8) };
        if ok == 0 {
            return None;
        }
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        buf.truncate(end);
        String::from_utf8(buf).ok()
    }

    fn i64(&self, key: &str) -> Option<i64> {
        let mut out = 0i64;
        let ok = unsafe { CFNumberGetValue(self.get(key)?, NUMBER_I64, (&raw mut out).cast()) };
        (ok != 0).then_some(out)
    }

    fn f64(&self, key: &str) -> Option<f64> {
        let mut out = 0f64;
        let ok = unsafe { CFNumberGetValue(self.get(key)?, NUMBER_F64, (&raw mut out).cast()) };
        (ok != 0).then_some(out)
    }

    fn bool(&self, key: &str) -> bool {
        self.get(key)
            .is_some_and(|b| unsafe { CFBooleanGetValue(b) } != 0)
    }

    fn rect(&self, key: &str) -> Option<Rect> {
        let mut rect = Rect::default();
        let ok = unsafe { CGRectMakeWithDictionaryRepresentation(self.get(key)?, &mut rect) };
        (ok != 0).then_some(rect)
    }
}

#[derive(Clone, Debug)]
pub struct CgWindow {
    pub id: u32,
    pub app: String,
    /// Empty unless the calling app may record the screen
    pub title: String,
    pub pid: i64,
    pub layer: i64,
    pub alpha: f64,
    pub on_screen: bool,
    pub frame: Rect,
}

fn window_list(option: u32) -> Vec<CgWindow> {
    let list = unsafe { CGWindowListCopyWindowInfo(option | WINDOW_LIST_EXCLUDE_DESKTOP, 0) };
    if list.is_null() {
        return vec![];
    }
    let list = Owned(list);
    let count = unsafe { CFArrayGetCount(list.0) };
    (0..count)
        .filter_map(|i| {
            let w = Dict(unsafe { CFArrayGetValueAtIndex(list.0, i) });
            Some(CgWindow {
                id: u32::try_from(w.i64("kCGWindowNumber")?).ok()?,
                app: w.string("kCGWindowOwnerName").unwrap_or_default(),
                title: w.string("kCGWindowName").unwrap_or_default(),
                pid: w.i64("kCGWindowOwnerPID")?,
                layer: w.i64("kCGWindowLayer").unwrap_or_default(),
                alpha: w.f64("kCGWindowAlpha").unwrap_or(1.0),
                on_screen: w.bool("kCGWindowIsOnscreen"),
                frame: w.rect("kCGWindowBounds")?,
            })
        })
        .collect()
}

/// All windows, in no particular order
pub fn windows() -> Vec<CgWindow> {
    window_list(WINDOW_LIST_ALL)
}

/// The windows on screen, front to back
pub fn windows_on_screen() -> Vec<CgWindow> {
    window_list(WINDOW_LIST_ON_SCREEN_ONLY)
}

pub struct CgDisplay {
    pub id: u32,
    pub frame: Rect,
    pub main: bool,
}

pub fn displays() -> Vec<CgDisplay> {
    let mut ids = [0u32; MAX_DISPLAYS];
    let mut count = 0u32;
    let err = unsafe { CGGetActiveDisplayList(MAX_DISPLAYS as u32, ids.as_mut_ptr(), &mut count) };
    if err != 0 {
        return vec![];
    }
    let main = unsafe { CGMainDisplayID() };
    ids[..count as usize]
        .iter()
        .map(|&id| CgDisplay {
            id,
            frame: unsafe { CGDisplayBounds(id) },
            main: id == main,
        })
        .collect()
}

/// Whether the human let this program record the screen, which window
/// titles need
pub fn may_record_screen() -> bool {
    // SAFETY: takes nothing, only reads the permission
    unsafe { CGPreflightScreenCaptureAccess() }
}

pub fn ask_to_record_screen() {
    // SAFETY: takes nothing; macOS shows its own prompt
    unsafe { CGRequestScreenCaptureAccess() };
}

pub fn ask_to_post_events() {
    // SAFETY: takes nothing; macOS shows its own prompt
    unsafe { CGRequestPostEventAccess() };
}

/// Input Monitoring, which a tap that only listens needs
pub fn may_listen_to_keys() -> bool {
    // SAFETY: takes nothing, only reads the permission
    unsafe { CGPreflightListenEventAccess() }
}

pub fn ask_to_listen_to_keys() {
    // SAFETY: takes nothing; macOS shows its own prompt
    unsafe { CGRequestListenEventAccess() };
}

/// Accessibility, which posting keys to an app needs
pub fn may_post_events() -> bool {
    // SAFETY: takes nothing, only reads the permission
    unsafe { CGPreflightPostEventAccess() }
}

/// An app or one of its windows, as the Accessibility API knows it
pub struct AxElement(Owned);

impl AxElement {
    pub fn app(pid: i64) -> Self {
        // SAFETY: returns a +1 element for any pid, live or not
        let app = AxElement(Owned(unsafe { AXUIElementCreateApplication(pid as i32) }));
        app.set_timeout();
        app
    }

    fn set_timeout(&self) {
        // SAFETY: valid element; applies to later calls on it
        unsafe { AXUIElementSetMessagingTimeout(self.0.0, AX_TIMEOUT.as_secs_f32()) };
    }

    fn attribute(&self, name: &str) -> Option<Owned> {
        let name = cf_string(name);
        let mut value = std::ptr::null();
        // SAFETY: valid element and name; value gets a +1 object on success
        let err = unsafe { AXUIElementCopyAttributeValue(self.0.0, name.0, &mut value) };
        (err == AX_SUCCESS && !value.is_null()).then(|| Owned(value))
    }

    fn element(&self, name: &str) -> Option<AxElement> {
        let element = AxElement(self.attribute(name)?);
        element.set_timeout();
        Some(element)
    }

    fn value<T: Default>(&self, name: &str, kind: u32) -> Option<T> {
        let value = self.attribute(name)?;
        let mut out = T::default();
        // SAFETY: out is the struct kind names; written only if they match
        let ok = unsafe { AXValueGetValue(value.0, kind, (&raw mut out).cast()) };
        (ok != 0).then_some(out)
    }

    pub fn set(&self, name: &str, value: bool) -> bool {
        let name = cf_string(name);
        // SAFETY: valid element and name; the booleans are static
        let value = unsafe {
            if value {
                kCFBooleanTrue
            } else {
                kCFBooleanFalse
            }
        };
        unsafe { AXUIElementSetAttributeValue(self.0.0, name.0, value) == AX_SUCCESS }
    }

    pub fn frame(&self) -> Option<Rect> {
        Some(Rect {
            origin: self.value("AXPosition", AX_VALUE_CGPOINT)?,
            size: self.value("AXSize", AX_VALUE_CGSIZE)?,
        })
    }

    pub fn focused_window(&self) -> Option<AxElement> {
        self.element("AXFocusedWindow")
    }

    /// The app's window at frame, as the window server places it
    pub fn window_at(&self, frame: Rect) -> Option<AxElement> {
        let windows = self.attribute("AXWindows")?;
        // SAFETY: checks the type before reading it as an array
        if unsafe { CFGetTypeID(windows.0) != CFArrayGetTypeID() } {
            return None;
        }
        let count = unsafe { CFArrayGetCount(windows.0) };
        (0..count)
            .map(|i| {
                // SAFETY: the array holds elements; retained for our Owned
                let window = AxElement(Owned(unsafe {
                    CFRetain(CFArrayGetValueAtIndex(windows.0, i))
                }));
                window.set_timeout();
                window
            })
            .find(|window| window.frame().is_some_and(|f| f.about(&frame)))
    }
}

impl PartialEq for AxElement {
    fn eq(&self, other: &Self) -> bool {
        // SAFETY: both are valid CF objects
        unsafe { CFEqual(self.0.0, other.0.0) != 0 }
    }
}

/// Posts a key down and up to one process only. Ok means it was posted;
/// what the app does with it can't be seen.
fn post_key(pid: i64, key: u16, text: Option<&[u16]>) -> Result<(), String> {
    for down in [true, false] {
        // SAFETY: a null source is allowed; Owned releases the event
        let event = unsafe { CGEventCreateKeyboardEvent(std::ptr::null(), key, down) };
        if event.is_null() {
            return Err("can't create a key event".into());
        }
        let event = Owned(event);
        // SAFETY: valid event; the call copies the text
        unsafe {
            if let Some(text) = text {
                CGEventKeyboardSetUnicodeString(event.0, text.len() as c_ulong, text.as_ptr());
            }
            CGEventPostToPid(pid as i32, event.0);
        }
        if down {
            std::thread::sleep(KEY_HOLD);
        }
    }
    Ok(())
}

/// Types text into a process as keyboard input
pub fn post_text(pid: i64, text: &str) -> Result<(), String> {
    let units: Vec<u16> = text.encode_utf16().collect();
    units
        .chunks(UNICODE_CHUNK)
        .try_for_each(|chunk| post_key(pid, 0, Some(chunk)))
}

pub fn post_return(pid: i64) -> Result<(), String> {
    post_key(pid, KEY_RETURN, None)
}

struct Pick {
    tap: CFTypeRef,
    run_loop: CFTypeRef,
    down: Option<Point>,
    click: Option<Point>,
}

extern "C" fn on_pick_event(
    _: *mut c_void,
    kind: u32,
    event: CFTypeRef,
    info: *mut c_void,
) -> CFTypeRef {
    // SAFETY: info is the Pick next_click lends for the run loop's life
    let pick = unsafe { &mut *info.cast::<Pick>() };
    match kind {
        LEFT_MOUSE_DOWN => pick.down = Some(unsafe { CGEventGetLocation(event) }),
        LEFT_MOUSE_UP if pick.down.is_some() => {
            pick.click = pick.down;
            unsafe { CFRunLoopStop(pick.run_loop) };
        }
        KEY_DOWN if unsafe { CGEventGetIntegerValueField(event, KEYCODE_FIELD) } == KEY_ESCAPE => {
            unsafe { CFRunLoopStop(pick.run_loop) };
        }
        TAP_DISABLED_BY_TIMEOUT => {
            unsafe { CGEventTapEnable(pick.tap, true) };
            return event;
        }
        _ => return event,
    }
    std::ptr::null()
}

/// Where the human clicks next, or None if they press Escape. The click
/// itself reaches no app.
pub fn next_click() -> Result<Option<Point>, String> {
    let mut pick = Pick {
        tap: std::ptr::null(),
        // SAFETY: the current thread's run loop, not owned
        run_loop: unsafe { CFRunLoopGetCurrent() },
        down: None,
        click: None,
    };
    let mask = [LEFT_MOUSE_DOWN, LEFT_MOUSE_UP, KEY_DOWN]
        .iter()
        .fold(0u64, |mask, kind| mask | 1 << kind);
    // SAFETY: pick outlives the run loop, the only time the tap calls back
    let tap = unsafe {
        CGEventTapCreate(
            SESSION_TAP,
            HEAD_INSERT,
            TAP_ACTIVE,
            mask,
            on_pick_event,
            (&raw mut pick).cast(),
        )
    };
    if tap.is_null() {
        return Err("can't watch clicks: hunch-host needs Accessibility".into());
    }
    let tap = Owned(tap);
    pick.tap = tap.0;
    // SAFETY: valid tap and run loop; Owned releases the source
    unsafe {
        let source = Owned(CFMachPortCreateRunLoopSource(std::ptr::null(), tap.0, 0));
        CFRunLoopAddSource(pick.run_loop, source.0, kCFRunLoopCommonModes);
        CFRunLoopRun();
        CGEventTapEnable(tap.0, false);
    }
    Ok(pick.click)
}

/// Seconds since the human last used a keyboard or pointer
pub fn idle() -> f64 {
    // SAFETY: only reads the window server's event times
    unsafe { CGEventSourceSecondsSinceLastEventType(COMBINED_SESSION_STATE, ANY_INPUT_EVENT) }
}

pub fn locked() -> bool {
    // SAFETY: returns a +1 dictionary, or null outside a login session
    let session = unsafe { CGSessionCopyCurrentDictionary() };
    if session.is_null() {
        return false;
    }
    let session = Owned(session);
    Dict(session.0).bool("CGSSessionScreenIsLocked")
}

/// Whether the human let this program record the microphone
pub fn may_record_audio() -> bool {
    // SAFETY: calls +[AVCaptureDevice authorizationStatusForMediaType:],
    // which takes a media type and returns an NSInteger
    unsafe {
        let class = objc_getClass(c"AVCaptureDevice".as_ptr());
        let selector = sel_registerName(c"authorizationStatusForMediaType:".as_ptr());
        let send: extern "C" fn(*const c_void, *const c_void, CFTypeRef) -> isize =
            std::mem::transmute(objc_msgSend as *const ());
        send(class, selector, AVMediaTypeAudio) == AV_AUTHORIZED
    }
}

/// A key the dictation listens for
pub enum Key {
    /// A modifier key changed: its keycode, and the flags it left
    Modifier {
        code: i64,
        flags: u64,
    },
    Escape,
    Other,
}

/// A key that dictates while held
pub struct DictationKey {
    pub name: &'static str,
    code: i64,
    /// The device flag of this key alone, apart from its twin on the
    /// other side
    flag: u64,
}

pub const DICTATION_KEYS: &[DictationKey] = &[
    DictationKey {
        name: "right-command",
        code: 54,
        flag: 0x10,
    },
    DictationKey {
        name: "right-option",
        code: 61,
        flag: 0x40,
    },
    DictationKey {
        name: "right-control",
        code: 62,
        flag: 0x2000,
    },
    DictationKey {
        name: "right-shift",
        code: 60,
        flag: 0x4,
    },
    DictationKey {
        name: "fn",
        code: 63,
        flag: 0x80_0000,
    },
];

impl DictationKey {
    /// Whether a modifier change pressed or released this key, if it's
    /// this one
    pub fn down(&self, key: &Key) -> Option<bool> {
        match key {
            Key::Modifier { code, flags } if *code == self.code => Some(flags & self.flag != 0),
            _ => None,
        }
    }
}

struct Keys {
    tap: CFTypeRef,
    on: Box<dyn FnMut(Key)>,
}

extern "C" fn on_key_event(
    _: *mut c_void,
    kind: u32,
    event: CFTypeRef,
    info: *mut c_void,
) -> CFTypeRef {
    // SAFETY: info is the Keys watch_keys keeps for the run loop's life
    let keys = unsafe { &mut *info.cast::<Keys>() };
    let code = unsafe { CGEventGetIntegerValueField(event, KEYCODE_FIELD) };
    match kind {
        FLAGS_CHANGED => {
            let flags = unsafe { CGEventGetFlags(event) };
            (keys.on)(Key::Modifier { code, flags });
        }
        KEY_DOWN if code == KEY_ESCAPE => (keys.on)(Key::Escape),
        KEY_DOWN => (keys.on)(Key::Other),
        TAP_DISABLED_BY_TIMEOUT => unsafe { CGEventTapEnable(keys.tap, true) },
        _ => {}
    }
    event
}

/// Calls on for each key the dictation listens for, until the process
/// ends. on runs on the tap's thread while the window server waits, so it
/// only hands the key on.
pub fn watch_keys(on: impl FnMut(Key) + 'static) -> Result<(), String> {
    let keys = Box::into_raw(Box::new(Keys {
        tap: std::ptr::null(),
        on: Box::new(on),
    }));
    let mask = [FLAGS_CHANGED, KEY_DOWN]
        .iter()
        .fold(0u64, |mask, kind| mask | 1 << kind);
    // SAFETY: keys lives as long as the process: it's never freed
    let tap = unsafe {
        CGEventTapCreate(
            SESSION_TAP,
            HEAD_INSERT,
            TAP_LISTEN,
            mask,
            on_key_event,
            keys.cast(),
        )
    };
    if tap.is_null() {
        // SAFETY: the tap that would use it was never made
        drop(unsafe { Box::from_raw(keys) });
        return Err("can't watch the dictation key: hunch-host needs Input Monitoring".into());
    }
    // SAFETY: valid tap and run loop; both live as long as the process
    unsafe {
        (*keys).tap = tap;
        let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
        CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopCommonModes);
        CFRunLoopRun();
    }
    Ok(())
}

pub fn pointer() -> Option<Point> {
    let event = unsafe { CGEventCreate(std::ptr::null()) };
    if event.is_null() {
        return None;
    }
    let event = Owned(event);
    Some(unsafe { CGEventGetLocation(event.0) })
}
