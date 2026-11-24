//! The tray's menu, as SEAT.md's Files say: every item with what it does,
//! so a tray only draws it.

use serde_json::{Value, json};

/// What the menu shows: what the human changes often. What they set
/// once, as where speech goes, is in the tray's settings
pub struct View {
  /// Running, or what the human has yet to grant
  pub status: String,
  /// The window dictation goes to, as the human knows it; None for the
  /// window in focus
  pub redirect: Option<String>,
}

fn title(title: impl Into<String>) -> Value {
  json!({"title": title.into()})
}

fn ctl(title: impl Into<String>, line: impl Into<String>) -> Value {
  json!({"title": title.into(), "ctl": line.into()})
}

fn separator(title: &str) -> Value {
  json!({"separator": title})
}

impl View {
  fn redirect(&self) -> Value {
    match &self.redirect {
      None => ctl("Redirect to Window…", "redirect"),
      Some(window) => ctl(format!("Stop Redirect to {window}"), "target focus"),
    }
  }

  pub fn menu(&self) -> Value {
    json!([
      title(&self.status),
      separator("Dictation"),
      {"title": "Copy Last Transcription", "copy": "last"},
      self.redirect(),
      separator(""),
      {"title": "Settings…", "settings": ""},
    ])
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn view() -> View {
    View {
      status: "Running".into(),
      redirect: None,
    }
  }

  fn find<'a>(menu: &'a Value, prefix: &str) -> &'a Value {
    menu
      .as_array()
      .unwrap()
      .iter()
      .find(|i| i["title"].as_str().is_some_and(|t| t.starts_with(prefix)))
      .unwrap_or_else(|| panic!("no item {prefix} in {menu}"))
  }

  #[test]
  fn the_menu_has_what_changes_often() {
    let menu = view().menu();
    let titles: Vec<&str> = menu
      .as_array()
      .unwrap()
      .iter()
      .filter_map(|i| i["title"].as_str())
      .collect();
    assert_eq!(
      titles,
      [
        "Running",
        "Copy Last Transcription",
        "Redirect to Window…",
        "Settings…"
      ]
    );
  }

  #[test]
  fn the_redirect_is_one_item() {
    assert_eq!(find(&view().menu(), "Redirect")["ctl"], "redirect");
    let menu = View {
      redirect: Some("iTerm2 — claude".into()),
      ..view()
    }
    .menu();
    assert_eq!(
      find(&menu, "Stop Redirect to iTerm2 — claude")["ctl"],
      "target focus"
    );
  }
}
