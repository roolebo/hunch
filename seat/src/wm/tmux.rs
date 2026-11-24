//! tmux as a wm: a tmux window is a WORKSPACE, a pane is a WINDOW, and
//! positions are in character cells.

use super::{Context, Display, Frame, State, Window, Wm, Workspace};
use crate::error::{Error, Result, failed};
use std::io::Write;
use std::process::{Command, Stdio};

const FIELDS: &[&str] = &[
  "pane_id",
  "pane_current_command",
  "pane_title",
  "pane_pid",
  "pane_tty",
  "window_id",
  "session_name",
  "window_index",
  "pane_left",
  "pane_top",
  "pane_width",
  "pane_height",
  "window_active",
  "session_attached",
  "window_zoomed_flag",
  "pane_active",
];

pub struct Tmux {
  socket: String,
}

/// A pane and the name of its tmux window, SESSION:INDEX
struct Pane {
  window: Window,
  workspace_name: String,
}

fn format() -> String {
  FIELDS
    .iter()
    .map(|f| format!("#{{{f}}}"))
    .collect::<Vec<_>>()
    .join("\t")
}

fn parse(line: &str, focused: Option<&str>) -> Option<Pane> {
  let f: Vec<&str> = line.split('\t').collect();
  let [
    id,
    app,
    title,
    pid,
    tty,
    workspace,
    session,
    index,
    left,
    top,
    width,
    height,
    window_active,
    session_attached,
    zoomed,
    pane_active,
  ] = f[..]
  else {
    return None;
  };
  let cells = |s: &str| s.parse::<f64>().ok();
  let shown = window_active == "1" && session_attached != "0";
  let zoomed = zoomed == "1";
  let active = pane_active == "1";
  Some(Pane {
    window: Window {
      id: id.into(),
      app: app.into(),
      title: title.into(),
      pid: pid.parse().ok(),
      tty: Some(tty.into()),
      host: None,
      workspace: Some(workspace.into()),
      frame: Frame {
        x: cells(left)?,
        y: cells(top)?,
        width: cells(width)?,
        height: cells(height)?,
      },
      visible: shown && (!zoomed || active),
      state: Some(if zoomed && active {
        State::Maximized
      } else {
        State::Normal
      }),
      focused: focused == Some(id),
    },
    workspace_name: format!("{session}:{index}"),
  })
}

impl Tmux {
  pub fn new(socket: &str) -> Self {
    Tmux {
      socket: socket.into(),
    }
  }

  fn tmux(&self, args: &[&str], input: Option<&str>) -> Result<String> {
    let mut child = Command::new("tmux")
      .arg("-S")
      .arg(&self.socket)
      .args(args)
      .stdin(if input.is_some() {
        Stdio::piped()
      } else {
        Stdio::null()
      })
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
      .spawn()
      .map_err(|e| failed(format!("can't run tmux: {e}")))?;
    if let Some(input) = input {
      let mut stdin = child.stdin.take().expect("stdin is piped");
      stdin
        .write_all(input.as_bytes())
        .map_err(|e| failed(format!("can't write to tmux: {e}")))?;
    }
    let output = child
      .wait_with_output()
      .map_err(|e| failed(format!("tmux didn't finish: {e}")))?;
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !output.status.success() {
      return Err(if stderr.starts_with("can't find pane") {
        Error::Gone(stderr)
      } else {
        failed(format!("tmux at {}: {stderr}", self.socket))
      });
    }
    Ok(
      String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_string(),
    )
  }

  /// The given client, or the one with the highest client_activity
  fn client(&self, given: Option<&str>) -> Result<String> {
    if let Some(tty) = given {
      return Ok(tty.into());
    }
    self
      .tmux(
        &["list-clients", "-F", "#{client_activity}\t#{client_tty}"],
        None,
      )?
      .lines()
      .filter_map(|line| line.split_once('\t'))
      .max_by_key(|(activity, _)| activity.parse::<u64>().unwrap_or(0))
      .map(|(_, tty)| tty.to_string())
      .ok_or_else(|| {
        failed(format!("no client is attached to tmux at {}", self.socket))
      })
  }

  fn focused_pane(&self, client: &str) -> Result<String> {
    self.tmux(&["display-message", "-p", "-c", client, "#{pane_id}"], None)
  }

  fn panes(&self, focused: Option<&str>) -> Result<Vec<Pane>> {
    Ok(
      self
        .tmux(&["list-panes", "-a", "-F", &format()], None)?
        .lines()
        .filter_map(|line| parse(line, focused))
        .collect(),
    )
  }

  /// Whether WINDOW's tmux window is zoomed, and whether WINDOW is its
  /// active pane
  fn zoom(&self, window: &str) -> Result<(bool, bool)> {
    let flags = self.tmux(
      &[
        "display-message",
        "-p",
        "-t",
        window,
        "#{window_zoomed_flag}#{pane_active}",
      ],
      None,
    )?;
    Ok((flags.starts_with('1'), flags.ends_with('1')))
  }

  fn toggle_zoom(&self, window: &str) -> Result<()> {
    self
      .tmux(&["resize-pane", "-Z", "-t", window], None)
      .map(drop)
  }
}

impl Wm for Tmux {
  fn scheme(&self) -> &'static str {
    "tmux"
  }

  fn context(&self, client: Option<&str>) -> Result<Context> {
    let client = self.client(client)?;
    let focused = self.focused_pane(&client)?;
    let pane = self
      .panes(Some(&focused))?
      .into_iter()
      .find(|p| p.window.focused)
      .ok_or_else(|| failed(format!("tmux lists no pane {focused}")))?;
    Ok(Context {
      wm: "tmux",
      workspace: Some(Workspace {
        id: pane.window.workspace.clone(),
        name: Some(pane.workspace_name),
        display: None,
      }),
      window: Some(pane.window),
      pointer: None,
    })
  }

  fn displays(&self) -> Result<Vec<Display>> {
    Ok(vec![])
  }

  fn windows(&self) -> Result<Vec<Window>> {
    // With no client attached, no pane has the focus
    let focused = self.client(None).and_then(|c| self.focused_pane(&c)).ok();
    Ok(
      self
        .panes(focused.as_deref())?
        .into_iter()
        .map(|p| p.window)
        .collect(),
    )
  }

  fn send(&self, window: &str, text: &str) -> Result<()> {
    let buffer = format!("hunch-{}", std::process::id());
    self.tmux(&["load-buffer", "-b", &buffer, "-"], Some(text))?;
    // -p: bracketed paste where the program asked for it, so a shell
    // or Vim takes the text as typed, not as commands
    let pasted = self.tmux(
      &["paste-buffer", "-d", "-p", "-b", &buffer, "-t", window],
      None,
    );
    if pasted.is_err() {
      let _ = self.tmux(&["delete-buffer", "-b", &buffer], None);
    }
    pasted.map(drop)
  }

  fn submit(&self, window: &str) -> Result<()> {
    self
      .tmux(&["send-keys", "-t", window, "Enter"], None)
      .map(drop)
  }

  fn focus(&self, window: &str) -> Result<()> {
    let client = self.client(None)?;
    self.tmux(&["switch-client", "-c", &client, "-t", window], None)?;
    self.tmux(&["select-pane", "-t", window], None).map(drop)
  }

  fn swap(&self, a: &str, b: &str) -> Result<()> {
    // tmux keeps the active pane at its place, so the keys would go to
    // the other program; the active one goes along with its pane
    let active: Vec<&str> = [a, b]
      .into_iter()
      .filter(|w| self.zoom(w).is_ok_and(|(_, active)| active))
      .collect();
    self.tmux(&["swap-pane", "-d", "-s", a, "-t", b], None)?;
    for w in active {
      self.tmux(&["select-pane", "-t", w], None)?;
    }
    Ok(())
  }

  fn place(&self, window: &str, frame: Frame) -> Result<()> {
    // A pane's position follows from its neighbours; only its size can
    // be asked for
    let width = frame.width.round().to_string();
    let height = frame.height.round().to_string();
    self
      .tmux(
        &["resize-pane", "-t", window, "-x", &width, "-y", &height],
        None,
      )
      .map(drop)
  }

  fn state(&self, window: &str, state: State) -> Result<()> {
    let (zoomed, active) = self.zoom(window)?;
    match state {
      State::Normal if zoomed => self.toggle_zoom(window),
      State::Normal => Ok(()),
      State::Maximized if zoomed && active => Ok(()),
      State::Maximized => {
        // Zooming another pane of a zoomed window first unzooms it
        if zoomed {
          self.toggle_zoom(window)?;
        }
        self.toggle_zoom(window)
      }
      State::Fullscreen | State::Minimized => Err(failed(format!(
        "tmux has only normal and maximized, not {}",
        state.name()
      ))),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn line(fields: &[&str]) -> String {
    fields.join("\t")
  }

  const PANE: &[&str] = &[
    "%3",
    "vim",
    "main.c",
    "4121",
    "/dev/pts/4",
    "@2",
    "hunch",
    "2",
    "0",
    "0",
    "80",
    "24",
    "1",
    "1",
    "0",
    "1",
  ];

  #[test]
  fn parse_reads_a_pane() {
    let pane = parse(&line(PANE), Some("%3")).unwrap();
    assert_eq!(pane.workspace_name, "hunch:2");
    let w = pane.window;
    assert_eq!(
      (w.id.as_str(), w.app.as_str(), w.pid),
      ("%3", "vim", Some(4121))
    );
    assert_eq!(
      w.frame,
      Frame {
        x: 0.0,
        y: 0.0,
        width: 80.0,
        height: 24.0
      }
    );
    assert!(w.visible && w.focused);
    assert_eq!(w.state, Some(State::Normal));
  }

  #[test]
  fn parse_hides_panes_of_windows_not_shown() {
    let mut fields = PANE.to_vec();
    fields[12] = "0";
    assert!(!parse(&line(&fields), None).unwrap().window.visible);
    let mut fields = PANE.to_vec();
    fields[13] = "0";
    assert!(!parse(&line(&fields), None).unwrap().window.visible);
  }

  #[test]
  fn parse_hides_panes_behind_a_zoomed_one() {
    let mut zoomed = PANE.to_vec();
    zoomed[14] = "1";
    let w = parse(&line(&zoomed), None).unwrap().window;
    assert_eq!((w.visible, w.state), (true, Some(State::Maximized)));
    zoomed[15] = "0";
    let w = parse(&line(&zoomed), None).unwrap().window;
    assert_eq!((w.visible, w.state), (false, Some(State::Normal)));
  }
}
