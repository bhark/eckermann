//! cargo run --release --example transcribe -- LANGUAGE MODEL.bin VAD.bin CLIP.wav [CLIP.wav ...]
//! PROMPT=text gives each clip that text as preceding context.

#[path = "common/wav.rs"]
mod wav;

use eckermann_lib::lang::Language;
use eckermann_lib::stt::Stt;
use std::path::Path;
use std::time::Instant;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: transcribe LANGUAGE MODEL.bin VAD.bin CLIP.wav [CLIP.wav ...]";
    let [language, model, vad, clips @ ..] = args.as_slice() else {
        return Err(usage.into());
    };
    let profile = match language.as_str() {
        "da" => Language::Da.profile(),
        "en" => Language::En.profile(),
        _ => return Err(usage.into()),
    };
    let t = Instant::now();
    let mut stt = Stt::load(Path::new(model), Path::new(vad), profile)?;
    println!("load {:.2}s", t.elapsed().as_secs_f32());
    let prompt = std::env::var("PROMPT").unwrap_or_default();
    for clip in clips {
        let pcm = wav::read_16k_mono(clip)?;
        let t = Instant::now();
        let text = stt.transcribe(&pcm, &prompt)?;
        println!(
            "{clip}: {:.1}s audio in {:.2}s\n  {text}",
            pcm.len() as f32 / 16_000.0,
            t.elapsed().as_secs_f32()
        );
    }
    Ok(())
}
