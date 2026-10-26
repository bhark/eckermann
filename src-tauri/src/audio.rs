//! Microphone capture on its own thread, since cpal streams are !Send on macOS.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample, Stream, StreamConfig};
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};
use std::sync::{Arc, Mutex, mpsc};

pub const RATE: u32 = 16_000;
const LEVELS_PER_SEC: u32 = 30;

type Levels = mpsc::SyncSender<f32>;
type Buffer = Arc<Mutex<Vec<f32>>>;

#[derive(serde::Serialize)]
pub struct Mic {
    id: String,
    name: String,
}

/// Microphones to choose from.
pub fn microphones() -> Vec<Mic> {
    let Ok(devices) = cpal::default_host().input_devices() else {
        return Vec::new();
    };
    devices
        .filter_map(|d| {
            Some(Mic {
                id: d.id().ok()?.to_string(),
                name: d.description().ok()?.name().to_owned(),
            })
        })
        // pulseaudio lists what each output plays as an input too
        .filter(|m| !m.id.ends_with(".monitor"))
        .collect()
}

enum Msg {
    Start(Option<String>, mpsc::Sender<Result<(), String>>),
    Drain(mpsc::Sender<Vec<f32>>),
    Stop(mpsc::Sender<Vec<f32>>),
}

struct Active {
    _stream: Stream,
    raw: Buffer,
    // swapped with raw on drain, so the callback keeps a grown buffer
    spare: Vec<f32>,
    to_16k: To16k,
}

impl Active {
    fn drain(&mut self) -> Vec<f32> {
        std::mem::swap(&mut *self.raw.lock().unwrap(), &mut self.spare);
        let pcm = self.to_16k.push(&self.spare);
        self.spare.clear();
        pcm
    }
}

pub struct Recorder {
    tx: mpsc::Sender<Msg>,
}

impl Recorder {
    /// `on_level` gets the RMS of the input about 30 times a second while recording.
    pub fn new(on_level: impl Fn(f32) + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        // bounded and try_send, so the audio callback never waits on the ui
        let (levels, level_rx) = mpsc::sync_channel(1);
        std::thread::spawn(move || level_rx.into_iter().for_each(on_level));
        std::thread::spawn(move || {
            let mut active: Option<Active> = None;
            for msg in rx {
                match msg {
                    Msg::Start(mic, reply) => {
                        let opened = open(mic.as_deref(), levels.clone());
                        let _ = reply.send(opened.map(|a| active = Some(a)));
                    }
                    Msg::Drain(reply) => {
                        let _ = reply.send(active.as_mut().map_or_else(Vec::new, Active::drain));
                    }
                    Msg::Stop(reply) => {
                        let pcm = active.take().map_or_else(Vec::new, |mut a| {
                            let mut pcm = a.drain();
                            pcm.extend(a.to_16k.finish());
                            pcm
                        });
                        let _ = reply.send(pcm);
                    }
                }
            }
        });
        Self { tx }
    }

    /// Opens `mic`, a cpal device id, or the default microphone when it is none or gone.
    pub fn start(&self, mic: Option<String>) -> Result<(), String> {
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(Msg::Start(mic, reply))
            .expect("recorder thread alive");
        rx.recv().expect("recorder thread alive")
    }

    /// 16 kHz mono heard since the last `drain` or `start`; keeps recording.
    pub fn drain(&self) -> Vec<f32> {
        self.ask(Msg::Drain)
    }

    /// Closes the microphone and returns the rest of what it heard as 16 kHz mono.
    pub fn stop(&self) -> Vec<f32> {
        self.ask(Msg::Stop)
    }

    fn ask(&self, msg: fn(mpsc::Sender<Vec<f32>>) -> Msg) -> Vec<f32> {
        let (reply, rx) = mpsc::channel();
        self.tx.send(msg(reply)).expect("recorder thread alive");
        rx.recv().expect("recorder thread alive")
    }
}

fn open(mic: Option<&str>, levels: Levels) -> Result<Active, String> {
    let host = cpal::default_host();
    let chosen = mic.and_then(|id| host.device_by_id(&id.parse().ok()?));
    if mic.is_some() && chosen.is_none() {
        log::warn!("microphone {mic:?} is gone; using the default");
    }
    let device = chosen
        .or_else(|| host.default_input_device())
        .ok_or("no microphone found")?;
    let supported = device.default_input_config().map_err(|e| e.to_string())?;
    let config = supported.config();
    // pulse's default record fragment holds back the first audio for about 2 s, then sends 170 ms blocks
    #[cfg(target_os = "linux")]
    let config = match supported.buffer_size() {
        cpal::SupportedBufferSize::Range { min, max } => StreamConfig {
            buffer_size: cpal::BufferSize::Fixed(
                (config.sample_rate / LEVELS_PER_SEC).clamp(*min, *max),
            ),
            ..config
        },
        cpal::SupportedBufferSize::Unknown => config,
    };
    let buf = Buffer::default();
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, &config, buf.clone(), levels),
        SampleFormat::I16 => build::<i16>(&device, &config, buf.clone(), levels),
        SampleFormat::I32 => build::<i32>(&device, &config, buf.clone(), levels),
        SampleFormat::U16 => build::<u16>(&device, &config, buf.clone(), levels),
        other => return Err(format!("unsupported microphone sample format {other}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(Active {
        _stream: stream,
        raw: buf,
        spare: Vec::new(),
        to_16k: To16k::new(config.sample_rate),
    })
}

fn build<T: SizedSample>(
    device: &cpal::Device,
    config: &StreamConfig,
    buf: Buffer,
    levels: Levels,
) -> Result<Stream, String>
where
    f32: FromSample<T>,
{
    let channels = config.channels as usize;
    let window = (config.sample_rate / LEVELS_PER_SEC) as usize;
    let (mut sum_sq, mut n) = (0.0_f32, 0);
    device
        .build_input_stream::<T, _, _>(
            *config,
            move |data: &[T], _| {
                let mut buf = buf.lock().unwrap();
                for frame in data.chunks(channels) {
                    let s =
                        frame.iter().map(|&x| f32::from_sample(x)).sum::<f32>() / channels as f32;
                    buf.push(s);
                    sum_sq += s * s;
                    n += 1;
                    if n == window {
                        let _ = levels.try_send((sum_sq / n as f32).sqrt());
                        (sum_sq, n) = (0.0, 0);
                    }
                }
            },
            |e| log::error!("microphone: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
}

/// Streaming conversion of device-rate mono to 16 kHz.
struct To16k {
    resampler: Option<Fft<f32>>,
    rate: u32,
    pending: Vec<f32>,
    scratch: Vec<f32>,
    // startup delay of the resampler, in output frames still to drop
    delay: usize,
    frames_in: usize,
    frames_out: usize,
}

impl To16k {
    fn new(rate: u32) -> Self {
        let resampler = (rate != RATE).then(|| {
            Fft::<f32>::new(rate as usize, RATE as usize, 1024, 1, FixedSync::Input)
                .expect("sample rates are non-zero")
        });
        let delay = resampler.as_ref().map_or(0, |r| r.output_delay());
        let scratch = vec![0.0; resampler.as_ref().map_or(0, |r| r.output_frames_max())];
        Self {
            resampler,
            rate,
            pending: Vec::new(),
            scratch,
            delay,
            frames_in: 0,
            frames_out: 0,
        }
    }

    fn push(&mut self, raw: &[f32]) -> Vec<f32> {
        self.frames_in += raw.len();
        self.process(raw)
    }

    /// Flushes the resampler so the total output matches the input duration.
    fn finish(&mut self) -> Vec<f32> {
        let expected = (self.frames_in as u64 * RATE as u64 / self.rate as u64) as usize;
        let mut out = Vec::new();
        while let Some(r) = &self.resampler
            && self.frames_out < expected
        {
            let silence = vec![0.0; r.input_frames_next()];
            out.extend(self.process(&silence));
        }
        out.truncate(
            out.len()
                .saturating_sub(self.frames_out.saturating_sub(expected)),
        );
        out
    }

    fn process(&mut self, raw: &[f32]) -> Vec<f32> {
        let Some(r) = &mut self.resampler else {
            self.frames_out += raw.len();
            return raw.to_vec();
        };
        self.pending.extend_from_slice(raw);
        let mut out = Vec::new();
        let mut used = 0;
        while self.pending.len() - used >= r.input_frames_next() {
            let n = r.input_frames_next();
            let input = InterleavedSlice::new(&self.pending[used..used + n], 1, n)
                .expect("mono slice matches its length");
            let frames = self.scratch.len();
            let mut output = InterleavedSlice::new_mut(&mut self.scratch, 1, frames)
                .expect("mono slice matches its length");
            let (read, written) = r
                .process_into_buffer(&input, &mut output, None)
                .expect("buffers sized by the resampler");
            used += read;
            let skip = self.delay.min(written);
            self.delay -= skip;
            out.extend_from_slice(&self.scratch[skip..written]);
        }
        self.pending.drain(..used);
        self.frames_out += out.len();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_keeps_duration_and_pitch() {
        for rate in [44_100, 48_000] {
            let tone: Vec<f32> = (0..rate)
                .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin())
                .collect();
            // uneven pieces, like audio callbacks deliver
            let mut to_16k = To16k::new(rate);
            let mut out = Vec::new();
            for piece in tone.chunks(rate as usize / 7) {
                out.extend(to_16k.push(piece));
            }
            out.extend(to_16k.finish());
            assert!(out.len() == RATE as usize, "{rate}: {} samples", out.len());
            let crossings = out
                .windows(2)
                .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
                .count();
            assert!(
                crossings.abs_diff(880) <= 2,
                "{rate}: {crossings} zero crossings"
            );
        }
    }
}
