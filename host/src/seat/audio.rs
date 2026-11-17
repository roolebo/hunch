//! Records the default microphone as compute takes audio: 16 kHz mono.

use crate::compute::wav::RATE;
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
    data.chunks(channels)
        .map(move |frame| frame.iter().map(|&s| to_f32(s)).sum::<f32>() / frame.len() as f32)
}

fn record(samples: Arc<Mutex<Vec<f32>>>) -> Result<(cpal::Stream, u32), String> {
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
            |e| eprintln!("hunch-host: the microphone failed: {e}"),
            None,
        )
        .map_err(|e| format!("can't record the microphone: {e}"))?;
    if !matches!(format, SampleFormat::F32 | SampleFormat::I16) {
        return Err(format!(
            "the microphone sends {format}, which hunch-host can't read yet"
        ));
    }
    stream
        .play()
        .map_err(|e| format!("can't start the microphone: {e}"))?;
    Ok((stream, config.sample_rate()))
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
