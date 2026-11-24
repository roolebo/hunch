//! The WAV local: takes for speech: 16-bit PCM, 16 kHz, mono. local:
//! reads it; a seat writes it for a model that takes it.

/// Its media type
pub const TYPE: &str = "audio/wav";
pub const RATE: u32 = 16_000;
const PCM: u16 = 1;

#[cfg(feature = "inference")]
fn u16_at(b: &[u8], at: usize) -> u16 {
  u16::from_le_bytes([b[at], b[at + 1]])
}

#[cfg(feature = "inference")]
fn u32_at(b: &[u8], at: usize) -> u32 {
  u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// The samples, in [-1, 1]
#[cfg(feature = "inference")]
pub fn parse(wav: &[u8]) -> Result<Vec<f32>, String> {
  if wav.len() < 12 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
    return Err("INPUT isn't a WAV file".into());
  }
  let mut at = 12;
  let mut format = None;
  while at + 8 <= wav.len() {
    let id = &wav[at..at + 4];
    let size = u32_at(wav, at + 4) as usize;
    let body = &wav[at + 8..(at + 8 + size).min(wav.len())];
    match id {
      b"fmt " if body.len() >= 16 => {
        format = Some((
          u16_at(body, 0),
          u16_at(body, 2),
          u32_at(body, 4),
          u16_at(body, 14),
        ));
      }
      b"data" => {
        if format != Some((PCM, 1, RATE, 16)) {
          return Err(format!(
            "INPUT is {format:?} (format, channels, rate, bits), not 16-bit \
             PCM at 16 kHz mono"
          ));
        }
        return Ok(
          body
            .as_chunks::<2>()
            .0
            .iter()
            .map(|s| f32::from(i16::from_le_bytes(*s)) / 32768.0)
            .collect(),
        );
      }
      _ => {}
    }
    // Chunks are padded to an even size
    at += 8 + size + size % 2;
  }
  Err("INPUT has no data".into())
}

/// pcm, samples in [-1, 1] at RATE, as such a WAV
#[cfg(feature = "seat")]
pub fn write(pcm: &[f32]) -> Vec<u8> {
  let data: Vec<u8> = pcm
    .iter()
    .flat_map(|s| ((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())
    .collect();
  let mut wav = Vec::with_capacity(44 + data.len());
  wav.extend(b"RIFF");
  wav.extend((36 + data.len() as u32).to_le_bytes());
  wav.extend(b"WAVEfmt ");
  wav.extend(16u32.to_le_bytes());
  wav.extend(PCM.to_le_bytes());
  // Mono: one channel, two bytes a frame
  wav.extend(1u16.to_le_bytes());
  wav.extend(RATE.to_le_bytes());
  wav.extend((RATE * 2).to_le_bytes());
  wav.extend(2u16.to_le_bytes());
  wav.extend(16u16.to_le_bytes());
  wav.extend(b"data");
  wav.extend((data.len() as u32).to_le_bytes());
  wav.extend(data);
  wav
}

#[cfg(test)]
mod tests {
  use super::*;

  fn wav(channels: u16, rate: u32, samples: &[i16]) -> Vec<u8> {
    let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    let mut w = Vec::new();
    w.extend(b"RIFF");
    w.extend((36 + data.len() as u32).to_le_bytes());
    w.extend(b"WAVEfmt ");
    w.extend(16u32.to_le_bytes());
    w.extend(PCM.to_le_bytes());
    w.extend(channels.to_le_bytes());
    w.extend(rate.to_le_bytes());
    w.extend((rate * 2 * u32::from(channels)).to_le_bytes());
    w.extend((2 * channels).to_le_bytes());
    w.extend(16u16.to_le_bytes());
    w.extend(b"data");
    w.extend((data.len() as u32).to_le_bytes());
    w.extend(data);
    w
  }

  #[test]
  fn parse_reads_samples() {
    assert_eq!(
      parse(&wav(1, RATE, &[0, 16384, -32768])).unwrap(),
      [0.0, 0.5, -1.0]
    );
  }

  #[cfg(feature = "seat")]
  #[test]
  fn write_makes_what_parse_reads() {
    assert_eq!(
      parse(&write(&[0.0, 0.5, -1.0])).unwrap(),
      [0.0, 16383.0 / 32768.0, -32767.0 / 32768.0]
    );
  }

  #[test]
  fn parse_refuses_other_formats() {
    assert!(parse(&wav(2, RATE, &[0, 0])).is_err());
    assert!(parse(&wav(1, 44_100, &[0])).is_err());
    assert!(parse(b"not a wav").is_err());
  }
}
