//! The CoreFoundation and CoreGraphics calls hunch-host needs on macOS.
//! Window and display positions are global: logical pixels from the top
//! left of the main display.

use std::ffi::{CString, c_char, c_long, c_void};

type CFTypeRef = *const c_void;
type CFIndex = c_long;

const UTF8: u32 = 0x0800_0100;
const NUMBER_I64: CFIndex = 4;
const NUMBER_F64: CFIndex = 13;
const WINDOW_LIST_ALL: u32 = 0;
const WINDOW_LIST_ON_SCREEN_ONLY: u32 = 1 << 0;
const WINDOW_LIST_EXCLUDE_DESKTOP: u32 = 1 << 4;
const MAX_DISPLAYS: usize = 16;

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
}

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

pub fn pointer() -> Option<Point> {
    let event = unsafe { CGEventCreate(std::ptr::null()) };
    if event.is_null() {
        return None;
    }
    let event = Owned(event);
    Some(unsafe { CGEventGetLocation(event.0) })
}
