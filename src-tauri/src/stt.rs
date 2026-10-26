use crate::audio::RATE;
use crate::lang::Profile;
use std::ffi::{c_int, c_void};
use std::ops::Range;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use whisper_rs::{
    DtwMode, DtwModelPreset, DtwParameters, FullParams, SamplingStrategy, WhisperContext,
    WhisperContextParameters, WhisperState, WhisperSysContext, WhisperSysState, WhisperTokenData,
    WhisperVadContext, WhisperVadContextParams, WhisperVadParams,
};

pub struct Stt {
    ctx: WhisperContext,
    state: WhisperState,
    // built once: whisper-rs leaks the CString set_language allocates
    params: FullParams<'static, 'static>,
    vad: Vad,
    filter: Filter,
}

/// Tokens the decoder may not pick.
struct Filter {
    // backslash starts NST tags like \Punktum, after which edda ends the text early
    tags: Vec<c_int>,
    // only right after a dot: without its tags edda can loop on ". . ."
    dots: Vec<c_int>,
}

impl Stt {
    /// `vad` is a Silero VAD model; without it whisper invents text from silence and noise.
    pub fn load(model: &Path, vad: &Path, profile: &'static Profile) -> Result<Self, String> {
        whisper_rs::install_logging_hooks();
        let mut cp = WhisperContextParameters::default();
        // edda is a turbo fine-tune, and turbo's alignment heads still time its words
        cp.dtw_parameters(DtwParameters {
            mode: DtwMode::ModelPreset {
                model_preset: DtwModelPreset::LargeV3Turbo,
            },
            ..Default::default()
        });
        let ctx = WhisperContext::new_with_params(utf8(model)?, cp)
            .map_err(|e| format!("load {}: {e}", model.display()))?;
        let state = ctx.create_state().map_err(|e| e.to_string())?;
        // greedy beat beam search 5 on FLEURS da: 8.1% vs 9.0% WER
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some(profile.code));
        params.set_no_timestamps(!profile.timestamps);
        params.set_suppress_nst(true);
        params.set_print_progress(false);
        let pick = |f: &dyn Fn(&[u8]) -> bool| -> Vec<c_int> {
            (0..ctx.token_eot())
                .filter(|&id| ctx.token_to_bytes(id).is_ok_and(f))
                .collect()
        };
        let filter = Filter {
            tags: pick(&|b| b.contains(&b'\\')),
            dots: pick(&|b| b == b"." || b == b" ."),
        };
        Ok(Self {
            ctx,
            state,
            params,
            vad: Vad::load(vad)?,
            filter,
        })
    }

    /// 16 kHz mono samples in, text out; empty when nobody spoke.
    /// `prompt` is the text just before this audio, so a phrase continues it.
    pub fn transcribe(&mut self, pcm: &[f32], prompt: &str) -> Result<String, String> {
        self.transcribe_unless(pcm, prompt, &AtomicBool::new(false))
    }

    /// As `transcribe`, but fails soon after `abort` is set.
    pub fn transcribe_unless(
        &mut self,
        pcm: &[f32],
        prompt: &str,
        abort: &AtomicBool,
    ) -> Result<String, String> {
        if pcm.is_empty() {
            return Ok(String::new());
        }
        let (speech, starts) = self.speech_only(pcm)?;
        if speech.is_empty() {
            return Ok(String::new());
        }
        let mut text = String::new();
        let mut from = 0;
        loop {
            let context = format!("{prompt} {text}");
            let (part, end) = self.decode(&speech[from..], context.trim(), abort)?;
            text = clean(&format!("{text} {part}"));
            // trained on one sentence per clip, edda often stops after the first
            match starts.iter().copied().find(|&s| s > from + end) {
                // a short tail is mostly the last word, which dtw times early
                Some(next) if !part.is_empty() && speech.len() - next >= RATE as usize => {
                    log::info!(
                        "resumed at {:.1}s of {:.1}s",
                        next as f32 / RATE as f32,
                        speech.len() as f32 / RATE as f32
                    );
                    from = next;
                }
                _ => return Ok(text),
            }
        }
    }

    /// Text of `speech`, and the sample where its last word ends.
    fn decode(
        &mut self,
        speech: &[f32],
        prompt: &str,
        abort: &AtomicBool,
    ) -> Result<(String, usize), String> {
        // tokens, not set_initial_prompt: whisper-rs leaks that CString on every call
        let tokens = match prompt {
            "" => Vec::new(),
            _ => self
                .ctx
                .tokenize(prompt, prompt.len() + 1)
                .map_err(|e| e.to_string())?,
        };
        let mut params = self.params.clone();
        params.set_tokens(&tokens);
        // safety: the callbacks only read the filter and the flag, which outlive this call
        unsafe {
            params.set_filter_logits_callback(Some(block));
            params
                .set_filter_logits_callback_user_data(&self.filter as *const Filter as *mut c_void);
            params.set_abort_callback(Some(aborted));
            params.set_abort_callback_user_data(abort as *const AtomicBool as *mut c_void);
        }
        self.state.full(params, speech).map_err(|e| e.to_string())?;
        let eot = self.ctx.token_eot();
        let mut end = None;
        let mut text = Vec::new();
        for seg in self.state.as_iter() {
            if let Ok(t) = seg.to_str_lossy() {
                text.push(t.into_owned());
            }
            for i in 0..seg.n_tokens() {
                let Some(tok) = seg.get_token(i) else {
                    continue;
                };
                let data = tok.token_data();
                if data.id < eot && data.t_dtw >= 0 {
                    // centiseconds
                    end = end.max(Some(data.t_dtw as usize * RATE as usize / 100));
                }
            }
        }
        Ok((clean(&text.join(" ")), end.unwrap_or(speech.len())))
    }

    // whisper.cpp only applies params.vad in whisper_full, not in the per-state call whisper-rs uses
    fn speech_only(&mut self, pcm: &[f32]) -> Result<(Vec<f32>, Vec<usize>), String> {
        let mut out = Vec::new();
        let mut starts = Vec::new();
        for range in self.vad.speech(pcm)? {
            if !out.is_empty() {
                // same 0.1 s gap whisper.cpp puts between speech segments
                out.resize(out.len() + RATE as usize / 10, 0.0);
            }
            starts.push(out.len());
            out.extend_from_slice(&pcm[range]);
        }
        Ok((out, starts))
    }
}

unsafe extern "C" fn block(
    _: *mut WhisperSysContext,
    _: *mut WhisperSysState,
    tokens: *const WhisperTokenData,
    n_tokens: c_int,
    logits: *mut f32,
    filter: *mut c_void,
) {
    let filter = unsafe { &*(filter as *const Filter) };
    let tokens = match n_tokens {
        0 => &[][..],
        n => unsafe { std::slice::from_raw_parts(tokens, n as usize) },
    };
    let after_dot = tokens.last().is_some_and(|t| filter.dots.contains(&t.id));
    let dots = if after_dot { &filter.dots[..] } else { &[] };
    for &id in filter.tags.iter().chain(dots) {
        unsafe { *logits.add(id as usize) = f32::NEG_INFINITY };
    }
}

unsafe extern "C" fn aborted(abort: *mut c_void) -> bool {
    unsafe { &*(abort as *const AtomicBool) }.load(Ordering::SeqCst)
}

pub struct Vad(WhisperVadContext);

impl Vad {
    pub fn load(path: &Path) -> Result<Self, String> {
        let mut params = WhisperVadContextParams::default();
        // silero is tiny; more threads only add spin barriers, which stall for seconds on a busy cpu
        params.set_n_threads(1);
        WhisperVadContext::new(utf8(path)?, params)
            .map(Self)
            .map_err(|e| format!("load {}: {e}", path.display()))
    }

    /// Sample ranges of 16 kHz `pcm` that hold speech.
    pub fn speech(&mut self, pcm: &[f32]) -> Result<Vec<Range<usize>>, String> {
        let found = self.scan(pcm)?;
        let mut out = Vec::new();
        for (i, range) in found.iter().enumerate() {
            out.push(range.clone());
            // silero's state after long speech can hide the next, seen at 0.44 against 0.9 fresh
            let next = found.get(i + 1).map_or(pcm.len(), |r| r.start);
            if next.saturating_sub(range.end) >= RATE as usize {
                let gap = self.speech(&pcm[range.end..next])?;
                out.extend(
                    gap.into_iter()
                        .map(|r| range.end + r.start..range.end + r.end),
                );
            }
        }
        Ok(out)
    }

    fn scan(&mut self, pcm: &[f32]) -> Result<Vec<Range<usize>>, String> {
        let segments = self
            .0
            .segments_from_samples(WhisperVadParams::default(), pcm)
            .map_err(|e| e.to_string())?;
        // timestamps are in centiseconds
        let at = |t: f32| ((t * RATE as f32 / 100.0) as usize).min(pcm.len());
        Ok(segments
            .map(|seg| at(seg.start)..at(seg.end).max(at(seg.start)))
            .collect())
    }
}

fn utf8(path: &Path) -> Result<&str, String> {
    path.to_str()
        .ok_or_else(|| format!("{} is not valid UTF-8", path.display()))
}

fn clean(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_whitespace() {
        assert_eq!(clean("  Hej  med\n dig. "), "Hej med dig.");
    }
}
