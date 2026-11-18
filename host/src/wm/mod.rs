//! The window manager role; skills/hunch/WM.md specifies it.

#[cfg(target_os = "linux")]
pub mod gnome;
#[cfg(target_os = "macos")]
mod macos;
mod tmux;

use crate::error::{Error, Result, failed};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::str::FromStr;

const USAGE: &str = "\
Usage: hunch-host wm ADDR COMMAND [ARGUMENTS]

Commands:
  context [--client TTY]         Where the human is: workspace, window, pointer
  displays                       All displays
  windows                        All windows
  send WINDOW                    Type stdin into WINDOW, keeping the focus
  submit WINDOW                  Press Enter in WINDOW, keeping the focus
  focus WINDOW                   Give WINDOW the focus
  swap WINDOW WINDOW             Exchange the places of two windows
  move WINDOW DIRECTION          Swap WINDOW with its neighbour: left, right, up, down
  place WINDOW X Y WIDTH HEIGHT  Put WINDOW there, or as near as the wm can
  state WINDOW STATE             Show WINDOW normal, maximized, fullscreen, minimized
  pick                           Let the human pick a window, print its id

TTY picks a multiplexer's client; it defaults to the one used last.";

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    Normal,
    Maximized,
    Fullscreen,
    Minimized,
}

impl State {
    fn name(self) -> &'static str {
        match self {
            State::Normal => "normal",
            State::Maximized => "maximized",
            State::Fullscreen => "fullscreen",
            State::Minimized => "minimized",
        }
    }
}

impl FromStr for State {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "normal" => Ok(State::Normal),
            "maximized" => Ok(State::Maximized),
            "fullscreen" => Ok(State::Fullscreen),
            "minimized" => Ok(State::Minimized),
            _ => Err(Error::Usage(format!("unknown STATE: {s}"), USAGE)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

impl FromStr for Direction {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "left" => Ok(Direction::Left),
            "right" => Ok(Direction::Right),
            "up" => Ok(Direction::Up),
            "down" => Ok(Direction::Down),
            _ => Err(Error::Usage(format!("unknown DIRECTION: {s}"), USAGE)),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Window {
    pub id: String,
    pub app: String,
    pub title: String,
    pub pid: Option<i64>,
    pub tty: Option<String>,
    pub host: Option<String>,
    pub workspace: Option<String>,
    pub frame: Frame,
    pub visible: bool,
    /// None when the wm can't tell, e.g. a macOS window off screen
    pub state: Option<State>,
    pub focused: bool,
}

impl Window {
    fn summary(&self) -> Value {
        json!({
            "id": self.id, "app": self.app, "title": self.title,
            "pid": self.pid, "tty": self.tty, "host": self.host,
        })
    }

    fn full(&self) -> Value {
        let mut value = self.summary();
        let extra = json!({
            "workspace": self.workspace,
            "x": self.frame.x, "y": self.frame.y,
            "width": self.frame.width, "height": self.frame.height,
            "state": self.state.map(State::name),
            "visible": self.visible, "focused": self.focused,
        });
        value
            .as_object_mut()
            .expect("summary is an object")
            .extend(extra.as_object().expect("extra is an object").clone());
        value
    }
}

pub struct Workspace {
    pub id: Option<String>,
    pub name: Option<String>,
    pub display: Option<String>,
}

pub struct Pointer {
    pub x: f64,
    pub y: f64,
    pub display: Option<String>,
    pub window: Option<String>,
}

pub struct Context {
    pub wm: &'static str,
    pub workspace: Option<Workspace>,
    pub window: Option<Window>,
    pub pointer: Option<Pointer>,
}

impl Context {
    fn to_json(&self) -> Value {
        json!({
            "wm": self.wm,
            "workspace": self.workspace.as_ref().map(|w| json!({
                "id": w.id, "name": w.name, "display": w.display,
            })),
            "window": self.window.as_ref().map(Window::summary),
            "pointer": self.pointer.as_ref().map(|p| json!({
                "x": p.x, "y": p.y, "display": p.display, "window": p.window,
            })),
        })
    }
}

pub struct Display {
    pub id: String,
    pub name: Option<String>,
    pub frame: Frame,
    pub main: bool,
}

impl Display {
    fn to_json(&self) -> Value {
        json!({
            "id": self.id, "name": self.name,
            "x": self.frame.x, "y": self.frame.y,
            "width": self.frame.width, "height": self.frame.height,
            "main": self.main,
        })
    }
}

/// A window manager. The commands that change something default to
/// failing, so a backend gains them one by one.
pub trait Wm {
    fn scheme(&self) -> &'static str;
    fn context(&self, client: Option<&str>) -> Result<Context>;
    fn displays(&self) -> Result<Vec<Display>>;
    fn windows(&self) -> Result<Vec<Window>>;

    fn send(&self, _window: &str, _text: &str) -> Result<()> {
        Err(self.not_yet("send"))
    }
    fn submit(&self, _window: &str) -> Result<()> {
        Err(self.not_yet("submit"))
    }
    fn focus(&self, _window: &str) -> Result<()> {
        Err(self.not_yet("focus"))
    }
    fn swap(&self, _a: &str, _b: &str) -> Result<()> {
        Err(self.not_yet("swap"))
    }
    fn place(&self, _window: &str, _frame: Frame) -> Result<()> {
        Err(self.not_yet("place"))
    }
    fn state(&self, _window: &str, _state: State) -> Result<()> {
        Err(self.not_yet("state"))
    }
    fn pick(&self) -> Result<String> {
        Err(self.not_yet("pick"))
    }

    fn not_yet(&self, command: &str) -> Error {
        failed(format!(
            "{command} isn't implemented for {} yet",
            self.scheme()
        ))
    }
}

pub fn open(addr: &str) -> Result<Box<dyn Wm>> {
    let (scheme, location) = addr
        .split_once(':')
        .ok_or_else(|| Error::Usage(format!("ADDR has no SCHEME: {addr}"), USAGE))?;
    match scheme {
        "tmux" => Ok(Box::new(tmux::Tmux::new(location))),
        #[cfg(target_os = "macos")]
        "macos" => Ok(Box::new(macos::Macos)),
        #[cfg(target_os = "linux")]
        "gnome" => Ok(Box::new(gnome::Gnome)),
        _ => Err(failed(format!("this build has no wm of SCHEME {scheme}"))),
    }
}

/// The nearest visible window of the same workspace in DIRECTION: it lies
/// wholly on that side and overlaps window across it.
fn neighbor<'a>(windows: &'a [Window], id: &str, direction: Direction) -> Result<&'a Window> {
    let window = windows
        .iter()
        .find(|w| w.id == id)
        .ok_or_else(|| Error::Gone(format!("no window {id}")))?;
    let a = window.frame;
    // How far b lies in direction, and how much it overlaps a across it
    let place = |b: Frame| match direction {
        Direction::Left => (a.x - (b.x + b.width), overlap(a.y, a.height, b.y, b.height)),
        Direction::Right => (b.x - (a.x + a.width), overlap(a.y, a.height, b.y, b.height)),
        Direction::Up => (a.y - (b.y + b.height), overlap(a.x, a.width, b.x, b.width)),
        Direction::Down => (b.y - (a.y + a.height), overlap(a.x, a.width, b.x, b.width)),
    };
    windows
        .iter()
        .filter(|w| w.id != id && w.visible && w.workspace == window.workspace)
        .map(|w| (w, place(w.frame)))
        .filter(|&(_, (gap, across))| gap >= 0.0 && across > 0.0)
        .min_by(|(_, (gap1, across1)), (_, (gap2, across2))| {
            gap1.total_cmp(gap2).then(across2.total_cmp(across1))
        })
        .map(|(w, _)| w)
        .ok_or_else(|| failed(format!("no window {direction:?} of {id}").to_lowercase()))
}

fn overlap(start1: f64, length1: f64, start2: f64, length2: f64) -> f64 {
    (start1 + length1).min(start2 + length2) - start1.max(start2)
}

fn number(s: &str) -> Result<f64> {
    s.parse()
        .map_err(|_| Error::Usage(format!("not a number: {s}"), USAGE))
}

/// Runs a wm command on input and out: stdin and stdout, or a client's in
/// the service
pub fn run(args: &[String], input: &mut dyn Read, out: &mut dyn Write) -> Result<()> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let (addr, command, rest) = match args.as_slice() {
        ["-h" | "--help", ..] => {
            return writeln!(out, "{USAGE}").map_err(|e| failed(format!("can't answer: {e}")));
        }
        [addr, command, rest @ ..] => (*addr, *command, rest),
        _ => return Err(Error::Usage("missing ADDR or COMMAND".into(), USAGE)),
    };
    let wm = open(addr)?;
    let said = match (command, rest) {
        ("context", []) => wm.context(None)?.to_json().to_string(),
        ("context", ["--client", tty]) => wm.context(Some(tty))?.to_json().to_string(),
        ("displays", []) => {
            let displays: Vec<Value> = wm.displays()?.iter().map(Display::to_json).collect();
            Value::from(displays).to_string()
        }
        ("windows", []) => {
            let windows: Vec<Value> = wm.windows()?.iter().map(Window::full).collect();
            Value::from(windows).to_string()
        }
        ("send", [window]) => {
            let mut text = String::new();
            input
                .read_to_string(&mut text)
                .map_err(|e| failed(format!("can't read TEXT from stdin: {e}")))?;
            wm.send(window, &text)?;
            format!("sent to {window}")
        }
        ("submit", [window]) => {
            wm.submit(window)?;
            format!("submitted in {window}")
        }
        ("focus", [window]) => {
            wm.focus(window)?;
            format!("focused {window}")
        }
        ("swap", [a, b]) => {
            wm.swap(a, b)?;
            format!("swapped {a} and {b}")
        }
        ("move", [window, direction]) => {
            let windows = wm.windows()?;
            let other = neighbor(&windows, window, direction.parse()?)?;
            wm.swap(window, &other.id)?;
            format!("swapped {window} and {}", other.id)
        }
        ("place", [window, x, y, width, height]) => {
            let frame = Frame {
                x: number(x)?,
                y: number(y)?,
                width: number(width)?,
                height: number(height)?,
            };
            wm.place(window, frame)?;
            format!("placed {window}")
        }
        ("state", [window, state]) => {
            wm.state(window, state.parse()?)?;
            format!("{window} is {state}")
        }
        ("pick", []) => wm.pick()?,
        _ => {
            return Err(Error::Usage(
                format!("unknown command or wrong arguments: {command}"),
                USAGE,
            ));
        }
    };
    writeln!(out, "{said}").map_err(|e| failed(format!("can't answer: {e}")))
}

/// Runs a command of gnome: in the service, which holds the extension's
/// connection; connect is what the extension runs to make it
#[cfg(target_os = "linux")]
pub fn run_here(
    args: &[String],
    input: &mut std::fs::File,
    out: &mut std::fs::File,
    _: &std::sync::atomic::AtomicBool,
) -> Result<()> {
    if args.get(1).is_some_and(|a| a == "connect") && args.len() == 2 {
        return gnome::connect(input, out);
    }
    run(args, input, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(id: &str, x: f64, y: f64, width: f64, height: f64) -> Window {
        Window {
            id: id.into(),
            workspace: Some("@1".into()),
            frame: Frame {
                x,
                y,
                width,
                height,
            },
            visible: true,
            ..Window::default()
        }
    }

    // Three columns, the right one split in two:
    //
    //   a | b | c
    //         |---
    //         | d
    fn layout() -> Vec<Window> {
        vec![
            window("a", 0.0, 0.0, 10.0, 20.0),
            window("b", 11.0, 0.0, 10.0, 20.0),
            window("c", 22.0, 0.0, 10.0, 9.0),
            window("d", 22.0, 10.0, 10.0, 10.0),
        ]
    }

    #[test]
    fn neighbor_takes_the_nearest_column() {
        assert_eq!(neighbor(&layout(), "b", Direction::Left).unwrap().id, "a");
        assert_eq!(neighbor(&layout(), "a", Direction::Right).unwrap().id, "b");
    }

    #[test]
    fn neighbor_prefers_the_larger_overlap() {
        let mut windows = layout();
        windows[2].frame.height = 4.0;
        windows[3].frame.y = 5.0;
        windows[3].frame.height = 15.0;
        assert_eq!(neighbor(&windows, "b", Direction::Right).unwrap().id, "d");
    }

    #[test]
    fn neighbor_goes_up_and_down() {
        assert_eq!(neighbor(&layout(), "d", Direction::Up).unwrap().id, "c");
        assert_eq!(neighbor(&layout(), "c", Direction::Down).unwrap().id, "d");
    }

    #[test]
    fn neighbor_skips_hidden_windows_and_other_workspaces() {
        let mut windows = layout();
        windows[0].visible = false;
        assert!(neighbor(&windows, "b", Direction::Left).is_err());
        let mut windows = layout();
        windows[0].workspace = Some("@2".into());
        assert!(neighbor(&windows, "b", Direction::Left).is_err());
    }

    #[test]
    fn neighbor_of_a_missing_window_is_gone() {
        assert!(matches!(
            neighbor(&layout(), "x", Direction::Left),
            Err(Error::Gone(_))
        ));
    }
}
