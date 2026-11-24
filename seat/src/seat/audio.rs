//! The microphones, as the OS has them.

use cpal::traits::{DeviceTrait, HostTrait};

/// A microphone, as devices lists it
pub struct Mic {
  pub id: String,
  pub name: String,
  pub in_use: bool,
}

fn listed(device: &cpal::Device) -> Option<Mic> {
  let id = device.id().ok()?.id().to_string();
  let name = device
    .description()
    .map_or_else(|_| id.clone(), |d| d.name().to_string());
  Some(Mic {
    id,
    name,
    in_use: false,
  })
}

/// The microphones the OS has; the default one is the one in use
pub fn mics() -> Result<Vec<Mic>, String> {
  #[cfg(target_os = "linux")]
  if let Some(mics) = sources() {
    return Ok(mics);
  }
  let host = cpal::default_host();
  let devices = host
    .input_devices()
    .map_err(|e| format!("can't list the microphones: {e}"))?;
  let mut mics: Vec<Mic> = devices.filter_map(|d| listed(&d)).collect();
  let Some(default) = host.default_input_device().and_then(|d| listed(&d))
  else {
    return Ok(mics);
  };
  // ALSA's default isn't among the devices it lists
  match mics.iter_mut().find(|m| m.id == default.id) {
    Some(m) => m.in_use = true,
    None => mics.insert(
      0,
      Mic {
        in_use: true,
        ..default
      },
    ),
  }
  Ok(mics)
}

/// The inputs of the sound server, PipeWire or PulseAudio, as the
/// desktop's settings show them, the one they pick in use; None without
/// a sound server, where ALSA's own devices are all there is. ALSA's
/// default records from the one in use.
#[cfg(target_os = "linux")]
fn sources() -> Option<Vec<Mic>> {
  let pactl = |args: &[&str]| {
    let out = std::process::Command::new("pactl")
      .args(args)
      .output()
      .ok()?;
    out.status.success().then_some(out.stdout)
  };
  let default = pactl(&["get-default-source"])?;
  let list = pactl(&["--format=json", "list", "sources"])?;
  Some(parse_sources(
    &list,
    String::from_utf8_lossy(&default).trim(),
  ))
}

/// The sources pactl lists as JSON, without the monitors of outputs
#[cfg(any(target_os = "linux", test))]
fn parse_sources(json: &[u8], default: &str) -> Vec<Mic> {
  let sources: Vec<serde_json::Value> =
    serde_json::from_slice(json).unwrap_or_default();
  sources
    .iter()
    .filter_map(|s| {
      let id = s["name"].as_str()?;
      if id.ends_with(".monitor") {
        return None;
      }
      Some(Mic {
        id: id.to_string(),
        name: s["description"].as_str().unwrap_or(id).to_string(),
        in_use: id == default,
      })
    })
    .collect()
}

#[cfg(test)]
mod tests {
  #[test]
  fn the_desktops_inputs_are_the_microphones() {
    let json = br#"[
      {"name": "alsa_output.pci.analog-stereo.monitor",
       "description": "Monitor of Built-in Audio"},
      {"name": "alsa_input.pci.analog-stereo",
       "description": "Built-in Audio Analog Stereo"},
      {"name": "bluez_input.headset", "description": "Headset"}
    ]"#;
    let mics = super::parse_sources(json, "bluez_input.headset");
    let shown: Vec<(&str, &str, bool)> = mics
      .iter()
      .map(|m| (m.id.as_str(), m.name.as_str(), m.in_use))
      .collect();
    assert_eq!(
      shown,
      [
        (
          "alsa_input.pci.analog-stereo",
          "Built-in Audio Analog Stereo",
          false
        ),
        ("bluez_input.headset", "Headset", true),
      ]
    );
  }
}
