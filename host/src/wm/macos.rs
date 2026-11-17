//! macOS as a wm. Spaces have no public API, so a workspace has neither
//! id nor name yet.

use super::{Context, Display, Frame, Pointer, State, Window, Wm, Workspace};
use crate::error::{Error, Result, failed};
use crate::macos::{self, AxElement, CgDisplay, CgWindow, Point, Rect};
use std::collections::HashMap;
use std::process::Command;

/// Ordinary app windows; menus, docks and overlays sit on other layers
const NORMAL_LAYER: i64 = 0;
const ITERM: &str = "iTerm2";

pub struct Macos;

fn frame(rect: Rect) -> Frame {
    Frame {
        x: rect.origin.x,
        y: rect.origin.y,
        width: rect.size.width,
        height: rect.size.height,
    }
}

fn is_app_window(w: &CgWindow) -> bool {
    w.layer == NORMAL_LAYER && w.alpha > 0.0
}

fn display_at(displays: &[CgDisplay], p: Point) -> Option<String> {
    displays
        .iter()
        .find(|d| d.frame.contains(p))
        .map(|d| d.id.to_string())
}

fn same_rect(a: Rect, b: Rect) -> bool {
    frame(a) == frame(b)
}

/// The tty of the current session of each iTerm2 window, by window id;
/// iTerm2's ids are the CoreGraphics ones. Inside the tell block, tab
/// would be iTerm2's tab class, so the separator is spelled out.
const ITERM_TTYS: &str = r#"
tell application "iTerm2"
    set out to ""
    repeat with w in windows
        set out to out & (id of w) & (character id 9) & (tty of current session of w) & linefeed
    end repeat
    return out
end tell"#;

/// iTerm2 takes keys only in its key window, which it lacks in the
/// background, so posted keys vanish; its scripting writes to a session
/// regardless.
const ITERM_WRITE: &str = r#"
on run argv
    tell application "iTerm2"
        tell current session of window id ((item 1 of argv) as integer)
            write text (item 2 of argv) newline false
        end tell
    end tell
end run"#;
/// iTerm2's own paste brackets text this way. Without the brackets, a
/// terminal agent takes the text for typing and a later Enter for a new
/// line.
const PASTE_START: &str = "\x1b[200~";
const PASTE_END: &str = "\x1b[201~";
/// write text's newline is a line feed, which a program in raw mode takes
/// for a new line, not for Enter
const ENTER: &str = "\r";

fn iterm_write(window: u32, text: &str) -> Result<()> {
    let output = Command::new("osascript")
        .args(["-e", ITERM_WRITE, &window.to_string(), text])
        .output()
        .map_err(|e| failed(format!("can't run osascript: {e}")))?;
    if !output.status.success() {
        return Err(failed(format!(
            "iTerm2 refused the text: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

fn find(window: &str) -> Result<CgWindow> {
    macos::windows()
        .into_iter()
        .find(|w| w.id.to_string() == window)
        .ok_or_else(|| Error::Gone(format!("no window {window}")))
}

/// Points the window's app at the window, posts keys, then gives the app
/// back the window it typed into. An app that doesn't answer Accessibility
/// gets the keys wherever it puts them.
fn deliver(
    window: &CgWindow,
    post: impl FnOnce(i64) -> std::result::Result<(), String>,
) -> Result<()> {
    let app = AxElement::app(window.pid);
    let previous = app.focused_window();
    let target = app
        .window_at(window.frame)
        .filter(|t| previous.as_ref() != Some(t));
    // AXMain and AXFocused order windows within the app only; the app
    // stays in the background
    let point = |w: &AxElement| {
        w.set("AXMain", true);
        w.set("AXFocused", true);
    };
    if let Some(target) = &target {
        point(target);
    }
    let posted = post(window.pid).map_err(failed);
    if let (Some(_), Some(previous)) = (&target, &previous) {
        point(previous);
    }
    posted
}

fn iterm_ttys() -> HashMap<String, String> {
    let Ok(output) = Command::new("osascript").args(["-e", ITERM_TTYS]).output() else {
        return HashMap::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .map(|(id, tty)| (id.to_string(), tty.to_string()))
        .collect()
}

impl Macos {
    /// The id of the front app window, which takes the keys
    fn front() -> Option<u32> {
        macos::windows_on_screen()
            .into_iter()
            .find(is_app_window)
            .map(|w| w.id)
    }

    fn window_at(p: Point) -> Option<CgWindow> {
        macos::windows_on_screen()
            .into_iter()
            .find(|w| is_app_window(w) && w.frame.contains(p))
    }
}

impl Wm for Macos {
    fn scheme(&self) -> &'static str {
        "macos"
    }

    fn context(&self, _client: Option<&str>) -> Result<Context> {
        let displays = macos::displays();
        let window = self.windows()?.into_iter().find(|w| w.focused);
        let workspace = window.as_ref().map(|w| Workspace {
            id: None,
            name: None,
            display: display_at(
                &displays,
                Point {
                    x: w.frame.x + w.frame.width / 2.0,
                    y: w.frame.y + w.frame.height / 2.0,
                },
            ),
        });
        let pointer = macos::pointer().map(|p| Pointer {
            x: p.x,
            y: p.y,
            display: display_at(&displays, p),
            window: Self::window_at(p).map(|w| w.id.to_string()),
        });
        Ok(Context {
            wm: "macos",
            workspace,
            window,
            pointer,
        })
    }

    fn displays(&self) -> Result<Vec<Display>> {
        Ok(macos::displays()
            .into_iter()
            .map(|d| Display {
                id: d.id.to_string(),
                // CoreGraphics has no display names; AppKit's NSScreen does
                name: None,
                frame: frame(d.frame),
                main: d.main,
            })
            .collect())
    }

    fn windows(&self) -> Result<Vec<Window>> {
        let displays = macos::displays();
        let front = Self::front();
        let windows: Vec<CgWindow> = macos::windows().into_iter().filter(is_app_window).collect();
        // Asking iTerm2 when it doesn't run would launch it
        let ttys = if windows.iter().any(|w| w.app == ITERM) {
            iterm_ttys()
        } else {
            HashMap::new()
        };
        Ok(windows
            .into_iter()
            .map(|w| {
                let fills_display = displays.iter().any(|d| same_rect(d.frame, w.frame));
                Window {
                    id: w.id.to_string(),
                    app: w.app,
                    title: w.title,
                    pid: Some(w.pid),
                    tty: ttys.get(&w.id.to_string()).cloned(),
                    host: None,
                    workspace: None,
                    frame: frame(w.frame),
                    visible: w.on_screen,
                    // Off screen, a window may be minimized, hidden or on
                    // another Space; CoreGraphics doesn't say which
                    state: w.on_screen.then_some(if fills_display {
                        State::Fullscreen
                    } else {
                        State::Normal
                    }),
                    focused: front == Some(w.id),
                }
            })
            .collect())
    }

    fn send(&self, window: &str, text: &str) -> Result<()> {
        let window = find(window)?;
        if window.app == ITERM {
            return iterm_write(window.id, &format!("{PASTE_START}{text}{PASTE_END}"));
        }
        deliver(&window, |pid| macos::post_text(pid, text))
    }

    fn submit(&self, window: &str) -> Result<()> {
        let window = find(window)?;
        if window.app == ITERM {
            return iterm_write(window.id, ENTER);
        }
        deliver(&window, macos::post_return)
    }

    fn pick(&self) -> Result<String> {
        eprintln!("click the window to pick, or press Escape");
        let click = macos::next_click()
            .map_err(failed)?
            .ok_or_else(|| Error::Cancelled("no window picked".into()))?;
        Self::window_at(click)
            .map(|w| w.id.to_string())
            .ok_or_else(|| Error::Cancelled("no window there".into()))
    }
}
