//! Dictation: record until toggled off, cutting at pauses so each phrase is transcribed
//! and pasted while the user keeps speaking. One worker transcribes the phrases in order,
//! and one typist pastes them in that order.

use crate::audio::{RATE, Recorder};
use crate::paste::Paster;
use crate::punct::{Label, Punctuator};
use crate::settings::Settings;
use crate::stt::{Stt, Vad};
use crate::text::{Edit, Joiner};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

pub const MODEL_FILE: &str = "ggml-edda-v0.2-q8_0.bin";
pub const VAD_FILE: &str = "ggml-silero-v6.2.0.bin";
pub const PUNCT_DIR: &str = "punct-da";

// shorter presses are accidental taps
const MIN_PRESS: Duration = Duration::from_millis(300);
const POLL: Duration = Duration::from_millis(200);
// speechless audio beyond this is dropped, so a silent session stays small
const MAX_SILENCE: usize = 10 * RATE as usize;
// audio before the last speech that vad scans again, so silero's state settles first
const SETTLE: usize = RATE as usize / 2;
const PREVIEW_EVERY: Duration = Duration::from_millis(700);
// whisper's window; a preview of a longer phrase shows only its end
const PREVIEW_MAX: usize = 28 * RATE as usize;

#[cfg(target_os = "macos")]
const PASTE_BY_HAND: &str = "Teksten ligger i udklipsholderen. Indsæt den med ⌘V.";
#[cfg(not(target_os = "macos"))]
const PASTE_BY_HAND: &str = "Teksten ligger i udklipsholderen. Indsæt den med Ctrl+V.";

/// What the overlay shows.
#[derive(Clone, serde::Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "lowercase")]
pub enum Event {
    Recording(bool),
    /// Input RMS, about 30 times a second while recording.
    Level(f32),
    /// Text so far of the phrase being spoken; empty once it is written.
    Preview(String),
    /// Phrases waiting to be written.
    Pending(usize),
    Notice(String),
}

enum Job {
    Warm,
    Preview(Vec<f32>),
    /// `last` ends the dictation.
    Phrase {
        pcm: Vec<f32>,
        last: bool,
    },
}

struct Session {
    started: Instant,
    pcm: Vec<f32>,
    phrases: usize,
    heard: Heard,
}

/// Speech in a session's pcm, found by scanning only the audio that may still change.
#[derive(Default)]
struct Heard {
    last: Option<Range<usize>>,
    // the next scan starts here; vad's verdict on the audio before is final
    from: usize,
    // end of the speech the last preview covered
    previewed: Option<usize>,
}

impl Heard {
    fn scan(&mut self, vad: &mut Vad, pcm: &[f32]) -> Result<(), String> {
        if let Some(found) = vad.speech(&pcm[self.from..])?.last() {
            self.last = Some(self.from + found.start..self.from + found.end);
        }
        // the last speech may still grow
        self.from = self
            .last
            .as_ref()
            .map_or(pcm.len(), |last| last.start)
            .saturating_sub(SETTLE);
        Ok(())
    }

    /// Follows the pcm losing its first `n` samples.
    fn drain(&mut self, n: usize) {
        self.last = self
            .last
            .take()
            .filter(|last| last.start >= n)
            .map(|last| last.start - n..last.end - n);
        self.from = self.from.saturating_sub(n);
        self.previewed = None;
    }
}

struct Live {
    recorder: Recorder,
    session: Mutex<Option<Session>>,
    settings: Arc<Mutex<Settings>>,
    jobs: mpsc::Sender<Job>,
    on_event: Arc<dyn Fn(Event) + Send + Sync>,
    pending: AtomicUsize,
    previewing: AtomicBool,
    // a running preview would hold up a queued phrase, which replaces its text anyway
    stale_preview: AtomicBool,
}

impl Live {
    fn queue_phrase(&self, pcm: Vec<f32>, last: bool) {
        log::info!(
            "queued {:.1}s, last: {last}",
            pcm.len() as f32 / RATE as f32
        );
        self.stale_preview.store(true, SeqCst);
        (self.on_event)(Event::Pending(self.pending.fetch_add(1, SeqCst) + 1));
        let _ = self.jobs.send(Job::Phrase { pcm, last });
    }

    fn phrase_done(&self) {
        (self.on_event)(Event::Pending(self.pending.fetch_sub(1, SeqCst) - 1));
    }

    fn notice(&self, text: &str) {
        (self.on_event)(Event::Notice(text.into()));
    }
}

pub struct Pipeline {
    live: Arc<Live>,
    wake: mpsc::Sender<()>,
}

impl Pipeline {
    pub fn new(
        models: PathBuf,
        settings: Arc<Mutex<Settings>>,
        on_event: impl Fn(Event) + Send + Sync + 'static,
    ) -> Self {
        let (jobs, job_rx) = mpsc::channel();
        let (wake, wake_rx) = mpsc::channel();
        let on_event: Arc<dyn Fn(Event) + Send + Sync> = Arc::new(on_event);
        let on_level = on_event.clone();
        let live = Arc::new(Live {
            recorder: Recorder::new(move |rms| on_level(Event::Level(rms))),
            session: Mutex::new(None),
            settings,
            jobs,
            on_event,
            pending: AtomicUsize::new(0),
            previewing: AtomicBool::new(false),
            stale_preview: AtomicBool::new(false),
        });
        let vad = models.join(VAD_FILE);
        let segmenter_live = live.clone();
        std::thread::spawn(move || segmenter(&segmenter_live, wake_rx, &vad));
        let (edits, edit_rx) = mpsc::channel();
        let typist_live = live.clone();
        std::thread::spawn(move || typist(&typist_live, edit_rx));
        let worker_live = live.clone();
        std::thread::spawn(move || worker(&worker_live, job_rx, edits, &models));
        Self { live, wake }
    }

    pub fn start(&self) {
        let mic = self.live.settings.lock().unwrap().mic.clone();
        let mut session = self.live.session.lock().unwrap();
        if session.is_some() {
            return;
        }
        match self.live.recorder.start(mic) {
            Ok(()) => {
                *session = Some(Session {
                    started: Instant::now(),
                    pcm: Vec::new(),
                    phrases: 0,
                    heard: Heard::default(),
                });
                (self.live.on_event)(Event::Recording(true));
                // load hides behind the speech
                let _ = self.live.jobs.send(Job::Warm);
                let _ = self.wake.send(());
            }
            Err(e) => {
                log::error!("microphone: {e}");
                self.live.notice("Mikrofonen kunne ikke startes.");
            }
        }
    }

    pub fn stop(&self) {
        let Some(mut session) = self.live.session.lock().unwrap().take() else {
            return;
        };
        session.pcm.extend(self.live.recorder.stop());
        (self.live.on_event)(Event::Recording(false));
        if session.phrases == 0 && session.started.elapsed() < MIN_PRESS {
            return;
        }
        self.live.queue_phrase(session.pcm, true);
    }

    /// Recording, or phrases still to write.
    pub fn busy(&self) -> bool {
        self.live.session.lock().unwrap().is_some() || self.live.pending.load(SeqCst) > 0
    }

    pub fn notice(&self, text: &str) {
        self.live.notice(text);
    }

    pub fn toggle(&self) {
        let recording = self.live.session.lock().unwrap().is_some();
        if recording { self.stop() } else { self.start() }
    }
}

fn segmenter(live: &Live, wake: mpsc::Receiver<()>, vad_path: &Path) {
    let mut vad = None;
    let mut last_preview = Instant::now();
    for () in wake {
        if vad.is_none() {
            vad = Vad::load(vad_path).inspect_err(|e| log::error!("{e}")).ok();
        }
        let Some(vad) = vad.as_mut() else {
            continue;
        };
        loop {
            std::thread::sleep(POLL);
            let mut guard = live.session.lock().unwrap();
            let Some(session) = guard.as_mut() else {
                break;
            };
            session.pcm.extend(live.recorder.drain());
            if let Err(e) = session.heard.scan(vad, &session.pcm) {
                log::error!("vad: {e}");
                continue;
            }
            let len = session.pcm.len();
            let pause = live.settings.lock().unwrap().pause_ms as usize * RATE as usize / 1000;
            // pause 0: write only when dictation stops
            let cut = (pause > 0)
                .then(|| phrase_end(session.heard.last.as_ref(), len, pause))
                .flatten();
            let speech_end = session.heard.last.as_ref().map(|last| last.end);
            if let Some(at) = cut {
                live.queue_phrase(session.pcm.drain(..at).collect(), false);
                session.heard.drain(at);
                session.phrases += 1;
            } else if speech_end.is_none() && len > MAX_SILENCE {
                session.pcm.drain(..len - RATE as usize);
                session.heard.drain(len - RATE as usize);
            } else if speech_end.is_some()
                && speech_end != session.heard.previewed
                && live.pending.load(SeqCst) == 0
                && !live.previewing.load(SeqCst)
                && last_preview.elapsed() >= PREVIEW_EVERY
            {
                // at most one preview at a time, none while a phrase waits, none of speech shown already
                live.previewing.store(true, SeqCst);
                live.stale_preview.store(false, SeqCst);
                session.heard.previewed = speech_end;
                last_preview = Instant::now();
                let pcm = session.pcm[len.saturating_sub(PREVIEW_MAX)..].to_vec();
                let _ = live.jobs.send(Job::Preview(pcm));
            }
        }
    }
}

/// Cut point inside the pause after the last speech, once that pause is `pause` samples long.
fn phrase_end(last: Option<&Range<usize>>, len: usize, pause: usize) -> Option<usize> {
    let last = last?;
    (len - last.end >= pause).then_some(last.end + pause / 2)
}

fn worker(live: &Live, rx: mpsc::Receiver<Job>, edits: mpsc::Sender<Edit>, models: &Path) {
    let mut stt: Option<Stt> = None;
    let mut punct: Option<Punctuator> = None;
    let mut joiner = Joiner::default();
    // development only: each phrase as audio and text, for examples/dictation_eval
    let keep = std::env::var_os("ECKERMANN_KEEP_AUDIO").map(PathBuf::from);
    let mut kept: Option<(u64, usize)> = None;
    loop {
        let idle = Duration::from_secs(60 * live.settings.lock().unwrap().unload_after_min as u64);
        let job = match stt {
            Some(_) => match rx.recv_timeout(idle) {
                Ok(job) => job,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    stt = None;
                    punct = None;
                    // glibc would keep the freed weights in its heap
                    #[cfg(all(target_os = "linux", target_env = "gnu"))]
                    unsafe {
                        libc::malloc_trim(0);
                    }
                    log::info!("speech model unloaded after {idle:?} idle");
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            },
            None => match rx.recv() {
                Ok(job) => job,
                Err(_) => return,
            },
        };
        if stt.is_none() {
            let t = Instant::now();
            match Stt::load(&models.join(MODEL_FILE), &models.join(VAD_FILE)) {
                Ok(loaded) => {
                    log::info!("speech model loaded in {:.2}s", t.elapsed().as_secs_f32());
                    stt = Some(loaded);
                }
                Err(e) => log::error!("{e}"),
            }
            // without it, dictation still works with whisper's own marks
            match Punctuator::load(&models.join(PUNCT_DIR)) {
                Ok(loaded) => punct = Some(loaded),
                Err(e) => log::error!("punctuation model: {e}"),
            }
        }
        let Some(model) = stt.as_mut() else {
            match job {
                Job::Warm => live.notice("Talemodellen kunne ikke indlæses."),
                Job::Preview(_) => live.previewing.store(false, SeqCst),
                Job::Phrase { .. } => live.phrase_done(),
            }
            continue;
        };
        match job {
            Job::Warm => {}
            Job::Preview(pcm) => {
                match model.transcribe_unless(&pcm, joiner.prompt(), &live.stale_preview) {
                    // a preview finishing after dictation stopped is stale
                    Ok(text) if live.session.lock().unwrap().is_some() => {
                        let preview = joiner.preview(&text, |w| labels(&punct, w));
                        (live.on_event)(Event::Preview(preview));
                    }
                    Ok(_) => {}
                    Err(_) if live.stale_preview.load(SeqCst) => {}
                    Err(e) => log::error!("preview: {e}"),
                }
                live.previewing.store(false, SeqCst);
            }
            Job::Phrase { pcm, last } => {
                let t = Instant::now();
                let text = model.transcribe(&pcm, joiner.prompt()).unwrap_or_else(|e| {
                    log::error!("transcribe: {e}");
                    String::new()
                });
                log::info!("transcribed in {:.2}s: {text:?}", t.elapsed().as_secs_f32());
                let trailing_space = live.settings.lock().unwrap().trailing_space;
                let edit = joiner.next(&text, last, trailing_space, |w| labels(&punct, w));
                if let Some(dir) = &keep {
                    let (dictation, n) = kept.get_or_insert_with(|| (unix_now(), 0));
                    if let Err(e) = keep_phrase(dir, *dictation, *n, &pcm, &text, &edit) {
                        log::error!("keep audio: {e}");
                    }
                    *n += 1;
                    if last {
                        kept = None;
                    }
                }
                let _ = edits.send(edit);
            }
        }
    }
}

/// Pastes each phrase's edit, apart from the worker: a paste waits for the app to fetch the clipboard.
fn typist(live: &Live, edits: mpsc::Receiver<Edit>) {
    let mut paster = Paster::new()
        .inspect_err(|e| log::error!("clipboard: {e}"))
        .ok();
    for edit in edits {
        if !edit.is_empty() {
            match paster.as_mut().map(|p| p.apply(&edit)) {
                Some(Ok(true)) => {}
                Some(Ok(false)) => live.notice(PASTE_BY_HAND),
                Some(Err(e)) => {
                    log::error!("paste: {e}");
                    live.notice("Teksten kunne ikke indsættes.");
                }
                None => {
                    log::error!("no clipboard; lost {edit:?}");
                    live.notice("Teksten kunne ikke indsættes.");
                }
            }
        }
        (live.on_event)(Event::Preview(String::new()));
        live.phrase_done();
    }
}

fn labels(punct: &Option<Punctuator>, words: &[String]) -> Vec<Label> {
    let Some(punct) = punct else {
        return Vec::new();
    };
    punct.label(words).unwrap_or_else(|e| {
        log::error!("punctuate: {e}");
        Vec::new()
    })
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Writes `<dictation>-<n>.wav` and adds a line to `<dictation>.txt`.
fn keep_phrase(
    dir: &Path,
    dictation: u64,
    n: usize,
    pcm: &[f32],
    raw: &str,
    edit: &Edit,
) -> Result<(), String> {
    use std::io::Write;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let path = dir.join(format!("{dictation}-{n}.wav"));
    let mut wav = hound::WavWriter::create(&path, spec).map_err(|e| e.to_string())?;
    for &s in pcm {
        wav.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
            .map_err(|e| e.to_string())?;
    }
    wav.finalize().map_err(|e| e.to_string())?;
    let mut txt = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(format!("{dictation}.txt")))
        .map_err(|e| e.to_string())?;
    writeln!(txt, "{n}\t{raw}\t{edit:?}").map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuts_inside_a_long_enough_pause() {
        let last = 2000..5000;
        assert_eq!(phrase_end(Some(&last), 5799, 800), None);
        assert_eq!(phrase_end(Some(&last), 5800, 800), Some(5400));
        assert_eq!(phrase_end(None, 99_999, 800), None);
    }
}
