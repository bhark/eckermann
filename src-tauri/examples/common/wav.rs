pub fn read_16k_mono(path: &str) -> Result<Vec<f32>, String> {
    let mut wav = hound::WavReader::open(path).map_err(|e| format!("{path}: {e}"))?;
    let spec = wav.spec();
    if spec.sample_rate != 16_000 || spec.channels != 1 {
        return Err(format!(
            "{path}: need 16 kHz mono, got {} Hz x{}",
            spec.sample_rate, spec.channels
        ));
    }
    let pcm: Result<Vec<f32>, _> = match spec.sample_format {
        hound::SampleFormat::Float => wav.samples::<f32>().collect(),
        hound::SampleFormat::Int => {
            let scale = (1_i64 << (spec.bits_per_sample - 1)) as f32;
            wav.samples::<i32>()
                .map(|s| s.map(|v| v as f32 / scale))
                .collect()
        }
    };
    pcm.map_err(|e| format!("{path}: {e}"))
}
