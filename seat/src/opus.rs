//! Ogg Opus, as RFC 7845 specifies it, for speech to a server: a tenth
//! of the WAV's size at the bitrate speech needs.

use crate::error::{Result, failed};
use crate::wav::RATE;
use ogg::writing::{PacketWriteEndInfo, PacketWriter};
use opus::{Application, Bitrate, Channels, Encoder};

/// Its media type
pub const TYPE: &str = "audio/ogg";
/// Plenty for speech: what servers transcribe it from stays intact
const BITRATE: i32 = 24_000;
/// 20 ms, the frame RFC 6716 suggests for speech
const FRAME: usize = RATE as usize / 50;
/// Ogg Opus counts its samples at 48 kHz, whatever the input's rate
const SCALE: u64 = 48_000 / RATE as u64;
/// Opus's largest packet
const MAX_PACKET: usize = 4000;
const SERIAL: u32 = 1;

fn opus_failed(e: opus::Error) -> crate::error::Error {
  failed(format!("can't encode the speech as Opus: {e}"))
}

/// The header packets: OpusHead, then OpusTags, with no tags
fn headers(pre_skip: u16) -> [Vec<u8>; 2] {
  let mut head = b"OpusHead".to_vec();
  head.push(1); // version
  head.push(1); // channels
  head.extend(pre_skip.to_le_bytes());
  head.extend(RATE.to_le_bytes());
  head.extend(0i16.to_le_bytes()); // output gain
  head.push(0); // channel mapping family: mono or stereo
  let vendor = b"hunch-seat";
  let mut tags = b"OpusTags".to_vec();
  tags.extend((vendor.len() as u32).to_le_bytes());
  tags.extend(vendor);
  tags.extend(0u32.to_le_bytes()); // user comments
  [head, tags]
}

/// PCM, 16 kHz mono in [-1, 1], as Ogg Opus
pub fn write(pcm: &[f32]) -> Result<Vec<u8>> {
  let mut encoder = Encoder::new(RATE, Channels::Mono, Application::Voip)
    .map_err(opus_failed)?;
  encoder
    .set_bitrate(Bitrate::Bits(BITRATE))
    .map_err(opus_failed)?;
  // The decoder drops the samples the encoder takes to warm up
  let lookahead = encoder.get_lookahead().map_err(opus_failed)? as u64;
  let pre_skip = lookahead * SCALE;
  let mut ogg = PacketWriter::new(Vec::new());
  let write = |ogg: &mut PacketWriter<Vec<u8>>, packet, end, at| {
    ogg
      .write_packet(packet, SERIAL, end, at)
      .map_err(|e| failed(format!("can't write Ogg: {e}")))
  };
  let [head, tags] = headers(pre_skip as u16);
  write(&mut ogg, head, PacketWriteEndInfo::EndPage, 0)?;
  write(&mut ogg, tags, PacketWriteEndInfo::EndPage, 0)?;
  // The encoder takes whole frames, so the last one is padded with
  // silence, and the end stays where the speech ends
  let frames = pcm.len().div_ceil(FRAME).max(1);
  let end = pre_skip + pcm.len() as u64 * SCALE;
  for n in 0..frames {
    let mut frame = pcm[(n * FRAME).min(pcm.len())..]
      .iter()
      .take(FRAME)
      .copied()
      .collect::<Vec<f32>>();
    frame.resize(FRAME, 0.0);
    let packet = encoder
      .encode_vec_float(&frame, MAX_PACKET)
      .map_err(opus_failed)?;
    let last = n + 1 == frames;
    let at = if last {
      end
    } else {
      pre_skip + ((n + 1) * FRAME) as u64 * SCALE
    };
    let info = if last {
      PacketWriteEndInfo::EndStream
    } else {
      PacketWriteEndInfo::NormalPacket
    };
    write(&mut ogg, packet, info, at)?;
  }
  Ok(ogg.into_inner())
}

#[cfg(test)]
mod tests {
  use super::*;
  use ogg::reading::PacketReader;
  use std::io::Cursor;

  fn tone(seconds: f32) -> Vec<f32> {
    (0..(RATE as f32 * seconds) as usize)
      .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / RATE as f32).sin())
      .collect()
  }

  #[test]
  fn the_stream_says_how_long_the_speech_is() {
    let pcm = tone(1.01);
    let ogg = write(&pcm).unwrap();
    let mut reader = PacketReader::new(Cursor::new(ogg));
    let head = reader.read_packet_expected().unwrap();
    assert!(head.data.starts_with(b"OpusHead"));
    let pre_skip = u16::from_le_bytes([head.data[10], head.data[11]]) as u64;
    assert!(
      reader
        .read_packet_expected()
        .unwrap()
        .data
        .starts_with(b"OpusTags")
    );
    let mut last = None;
    while let Some(packet) = reader.read_packet().unwrap() {
      last = Some(packet);
    }
    let last = last.unwrap();
    assert!(last.last_in_stream());
    assert_eq!(last.absgp_page() - pre_skip, pcm.len() as u64 * SCALE);
  }

  #[test]
  fn opus_is_far_smaller_than_wav() {
    let pcm = tone(3.0);
    assert!(write(&pcm).unwrap().len() * 5 < crate::wav::write(&pcm).len());
  }
}
