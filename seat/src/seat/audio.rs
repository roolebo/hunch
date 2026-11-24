//! Records the default microphone as models take speech: 16 kHz mono.

use crate::wav::RATE;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Data, SampleFormat};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

/// A recording in progress. A stream may have to stay on the thread that
/// made it, so a thread of its own holds it.
pub struct Recording {
  stop: mpsc::Sender<()>,
  done: mpsc::Receiver<Vec<f32>>,
}

/// Each frame's channels averaged into one sample
fn mono<T: Copy>(
  data: &[T],
  channels: usize,
  to_f32: impl Fn(T) -> f32,
) -> impl Iterator<Item = f32> {
  data.chunks(channels).map(move |frame| {
    frame.iter().map(|&s| to_f32(s)).sum::<f32>() / frame.len() as f32
  })
}

fn record(
  samples: Arc<Mutex<Vec<f32>>>,
) -> Result<(cpal::Stream, u32), String> {
  let device = cpal::default_host()
    .default_input_device()
    .ok_or("there's no microphone")?;
  let config = device
    .default_input_config()
    .map_err(|e| format!("can't ask the microphone for its format: {e}"))?;
  let channels = usize::from(config.channels());
  let format = config.sample_format();
  let stream = device
    .build_input_stream_raw(
      config.config(),
      format,
      move |data: &Data, _: &_| {
        let mut samples = samples.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(data) = data.as_slice::<f32>() {
          samples.extend(mono(data, channels, |s| s));
        } else if let Some(data) = data.as_slice::<i16>() {
          samples.extend(mono(data, channels, |s| f32::from(s) / 32768.0));
        }
      },
      |e| eprintln!("hunch-seat: the microphone failed: {e}"),
      None,
    )
    .map_err(|e| format!("can't record the microphone: {e}"))?;
  if !matches!(format, SampleFormat::F32 | SampleFormat::I16) {
    return Err(format!(
      "the microphone sends {format}, which hunch-seat can't read yet"
    ));
  }
  stream
    .play()
    .map_err(|e| format!("can't start the microphone: {e}"))?;
  Ok((stream, config.sample_rate()))
}

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

/// Samples at rate brought to RATE: from a whole multiple by averaging
/// blocks, a crude low-pass enough for speech models; from any other rate
/// by interpolating, with no low-pass.
pub fn resample(input: &[f32], rate: u32) -> Vec<f32> {
  if rate == RATE {
    return input.to_vec();
  }
  if rate.is_multiple_of(RATE) {
    let block = (rate / RATE) as usize;
    return input
      .chunks(block)
      .map(|b| b.iter().sum::<f32>() / b.len() as f32)
      .collect();
  }
  let step = f64::from(rate) / f64::from(RATE);
  let len = (input.len() as f64 / step) as usize;
  (0..len)
    .map(|i| {
      let x = i as f64 * step;
      let j = x as usize;
      let a = input[j];
      let b = input.get(j + 1).copied().unwrap_or(a);
      a + (b - a) * (x - j as f64) as f32
    })
    .collect()
}

impl Recording {
  pub fn start() -> Result<Self, String> {
    let (stop, stopped) = mpsc::channel();
    let (finish, done) = mpsc::channel();
    let (started, start) = mpsc::channel();
    thread::spawn(move || {
      let samples = Arc::new(Mutex::new(Vec::new()));
      let (stream, rate) = match record(Arc::clone(&samples)) {
        Ok(recording) => recording,
        Err(e) => {
          let _ = started.send(Err(e));
          return;
        }
      };
      let _ = started.send(Ok(()));
      // A dropped Recording, a cancel, ends the wait as stop does
      let _ = stopped.recv();
      drop(stream);
      let samples = samples.lock().unwrap_or_else(|e| e.into_inner());
      let _ = finish.send(resample(&samples, rate));
    });
    start
      .recv()
      .map_err(|_| "the recording thread ended".to_string())??;
    Ok(Recording { stop, done })
  }

  pub fn stop(self) -> Vec<f32> {
    let _ = self.stop.send(());
    self.done.recv().unwrap_or_default()
  }
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

  use super::*;

  #[test]
  fn resample_averages_blocks() {
    assert_eq!(
      resample(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 48_000),
      [2.0, 5.0]
    );
  }

  #[test]
  fn resample_interpolates_other_rates() {
    let out = resample(&[0.0, 1.0, 2.0, 3.0], 24_000);
    assert_eq!(out.len(), 2);
    assert_eq!(out[1], 1.5);
  }

  #[test]
  fn mono_averages_channels() {
    let out: Vec<f32> = mono(&[1.0, 3.0, -1.0, 1.0], 2, |s| s).collect();
    assert_eq!(out, [2.0, 0.0]);
  }
}
