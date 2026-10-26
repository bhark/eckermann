//! cargo run --release --example record -- SECONDS OUT.wav

use eckermann_lib::audio::{RATE, Recorder};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

static PEAK: AtomicU32 = AtomicU32::new(0);

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [secs, out] = args.as_slice() else {
        return Err("usage: record SECONDS OUT.wav".into());
    };
    let secs: f32 = secs.parse().map_err(|e| format!("{secs}: {e}"))?;
    let rec = Recorder::new(|level| {
        PEAK.fetch_max(level.to_bits(), Ordering::Relaxed);
    });
    rec.start()?;
    std::thread::sleep(Duration::from_secs_f32(secs));
    let pcm = rec.stop();
    let peak = f32::from_bits(PEAK.load(Ordering::Relaxed));
    println!(
        "{:.2}s at 16 kHz, peak level {peak:.4}",
        pcm.len() as f32 / RATE as f32
    );
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: RATE,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut wav = hound::WavWriter::create(out, spec).map_err(|e| e.to_string())?;
    for s in pcm {
        wav.write_sample(s).map_err(|e| e.to_string())?;
    }
    wav.finalize().map_err(|e| e.to_string())
}
