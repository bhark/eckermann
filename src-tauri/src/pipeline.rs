//! Dictation: record until toggled off, typing each phrase's words as soon as two previews
//! agree on them, and the rest once a pause ends the phrase. One worker transcribes in
//! order, and one typist pastes in that order.

use crate::audio::{RATE, Recorder};
use crate::models;
use crate::paste::Paster;
use crate::punct::{Label, Punctuator};
use crate::settings::Settings;
use crate::stt::{Stt, Vad};
use crate::text::{Edit, Joiner};
use crate::ui::{Text, text};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

// shorter presses are accidental taps
const MIN_PRESS: Duration = Duration::from_millis(300);
const POLL: Duration = Duration::from_millis(200);
// speechless audio beyond this is dropped, so a silent session stays small
const MAX_SILENCE: usize = 10 * RATE as usize;
// audio before the last speech that vad scans again, so silero's state settles first
const SETTLE: usize = RATE as usize / 2;
// whisper's window; a preview of a longer phrase shows only its end
const PREVIEW_MAX: usize = 28 * RATE as usize;
// a phrase this long is cut where its last speech began, so previews keep seeing it whole
const LONG_PHRASE: usize = 20 * RATE as usize;
// silence after which a phrase's held mark is typed, as the user may edit next
const MARK_AFTER: Duration = Duration::from_secs(2);

/// What the overlay shows.
#[derive(Clone, serde::Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "lowercase")]
pub enum Event {
    Recording(bool),
    /// Input RMS, about 30 times a second while recording.
    Level(f32),
    /// Text of the phrase being spoken that is not typed yet.
    Preview(String),
    /// Phrases waiting to be written.
    Pending(usize),
    Notice(String),
}

enum Job {
    Warm,
    /// `early`: words may be typed before the pause, as the audio starts where the phrase does.
    Preview {
        pcm: Vec<f32>,
        early: bool,
    },
    /// `last` ends the dictation.
    Phrase {
        pcm: Vec<f32>,
        last: bool,
    },
    Settle,
}

struct Session {
    started: Instant,
    pcm: Vec<f32>,
    phrases: usize,
    heard: Heard,
    mark_due: Option<Instant>,
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
        let vad = models.join(models::VAD.path);
        let segmenter_live = live.clone();
        std::thread::spawn(move || segmenter(&segmenter_live, wake_rx, &vad));
        let (writes, write_rx) = mpsc::channel();
        let typist_live = live.clone();
        std::thread::spawn(move || typist(&typist_live, write_rx));
        let worker_live = live.clone();
        std::thread::spawn(move || worker(&worker_live, job_rx, writes, &models));
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
                    mark_due: None,
                });
                (self.live.on_event)(Event::Recording(true));
                // load hides behind the speech
                let _ = self.live.jobs.send(Job::Warm);
                let _ = self.wake.send(());
            }
            Err(e) => {
                log::error!("microphone: {e}");
                self.live.notice(text(Text::MicFailed));
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
            if speech_end.is_some() {
                session.mark_due = None;
            } else if session.mark_due.is_some_and(|due| Instant::now() >= due) {
                session.mark_due = None;
                let _ = live.jobs.send(Job::Settle);
            }
            if let Some(at) = cut {
                live.queue_phrase(session.pcm.drain(..at).collect(), false);
                session.heard.drain(at);
                session.phrases += 1;
                // the cut comes a pause after the speech
                let pause = Duration::from_secs_f32(pause as f32 / RATE as f32);
                session.mark_due = Some(Instant::now() + MARK_AFTER.saturating_sub(pause));
            } else if speech_end.is_none() && len > MAX_SILENCE {
                session.pcm.drain(..len - RATE as usize);
                session.heard.drain(len - RATE as usize);
            } else if speech_end.is_some()
                && speech_end != session.heard.previewed
                && live.pending.load(SeqCst) == 0
                && !live.previewing.load(SeqCst)
            {
                // at most one preview at a time, none while a phrase waits, none of speech shown already
                live.previewing.store(true, SeqCst);
                live.stale_preview.store(false, SeqCst);
                session.heard.previewed = speech_end;
                let from = len.saturating_sub(PREVIEW_MAX);
                let _ = live.jobs.send(Job::Preview {
                    pcm: session.pcm[from..].to_vec(),
                    // pause 0 promises nothing typed before dictation stops
                    early: from == 0 && pause > 0,
                });
            }
        }
    }
}

/// Cut point inside the pause after the last speech, once that pause is `pause` samples long;
/// in a long phrase, where its last speech began after a shorter one.
fn phrase_end(last: Option<&Range<usize>>, len: usize, pause: usize) -> Option<usize> {
    let last = last?;
    if len - last.end >= pause {
        return Some(last.end + pause / 2);
    }
    (len >= LONG_PHRASE && last.start >= LONG_PHRASE / 2).then_some(last.start)
}

fn worker(live: &Live, rx: mpsc::Receiver<Job>, writes: mpsc::Sender<Write>, models: &Path) {
    let mut stt: Option<Stt> = None;
    let mut punct: Option<Punctuator> = None;
    let mut language = live.settings.lock().unwrap().language;
    let mut joiner = Joiner::new(language.profile());
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
        // a new language takes effect when a dictation starts, never inside one
        let wanted = live.settings.lock().unwrap().language;
        if matches!(job, Job::Warm) {
            if wanted != language {
                log::info!("dictating in {wanted:?} from now on");
                (stt, punct, language) = (None, None, wanted);
            }
            // the page may have changed since the last dictation, so nothing typed then is trusted
            joiner = Joiner::new(language.profile());
        }
        if stt.is_none() {
            let t = Instant::now();
            let profile = language.profile();
            let speech = models.join(profile.speech.path);
            match Stt::load(&speech, &models.join(models::VAD.path), profile) {
                Ok(loaded) => {
                    log::info!("speech model loaded in {:.2}s", t.elapsed().as_secs_f32());
                    stt = Some(loaded);
                }
                Err(e) => log::error!("{e}"),
            }
            // without it, dictation still works with whisper's own marks
            match Punctuator::load(&models.join(profile.punct_dir)) {
                Ok(loaded) => punct = Some(loaded),
                Err(e) => log::error!("punctuation model: {e}"),
            }
        }
        let Some(model) = stt.as_mut() else {
            match job {
                Job::Warm => live.notice(text(Text::ModelFailed)),
                Job::Preview { .. } => live.previewing.store(false, SeqCst),
                Job::Phrase { .. } => live.phrase_done(),
                Job::Settle => {}
            }
            continue;
        };
        match job {
            Job::Warm => {}
            Job::Settle => {
                let _ = writes.send(Write {
                    edit: joiner.settle(),
                    preview: Some(String::new()),
                });
            }
            Job::Preview { pcm, early } => {
                let t = Instant::now();
                let result = model.transcribe_unless(&pcm, joiner.prompt(), &live.stale_preview);
                log::info!(
                    "previewed {:.1}s in {:.2}s",
                    pcm.len() as f32 / RATE as f32,
                    t.elapsed().as_secs_f32()
                );
                let current =
                    live.session.lock().unwrap().is_some() && !live.stale_preview.load(SeqCst);
                match result {
                    // a preview finishing after its phrase ended or dictation stopped is stale
                    Ok(text) if current => {
                        let label = |w: &[String]| labels(&punct, w);
                        let (edit, preview) = match early {
                            true => joiner.heard(&text, label),
                            // ponytail: speech with no gap past whisper's window types only at the pause
                            false => (Edit::default(), joiner.preview(&text, label)),
                        };
                        let _ = writes.send(Write {
                            edit,
                            preview: Some(preview),
                        });
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
                let _ = writes.send(Write {
                    edit,
                    preview: None,
                });
            }
        }
    }
}

/// An edit for the typist, then the preview to show; none ends a phrase.
struct Write {
    edit: Edit,
    preview: Option<String>,
}

/// Pastes each edit, apart from the worker: a paste waits for the app to fetch the clipboard.
fn typist(live: &Live, writes: mpsc::Receiver<Write>) {
    let mut paster = Paster::new()
        .inspect_err(|e| log::error!("clipboard: {e}"))
        .ok();
    for Write { edit, preview } in writes {
        if !edit.is_empty() {
            match paster.as_mut().map(|p| p.apply(&edit)) {
                Some(Ok(true)) => {}
                Some(Ok(false)) => live.notice(text(Text::PasteByHand)),
                Some(Err(e)) => {
                    log::error!("paste: {e}");
                    live.notice(text(Text::PasteFailed));
                }
                None => {
                    log::error!("no clipboard; lost {edit:?}");
                    live.notice(text(Text::PasteFailed));
                }
            }
        }
        let done = preview.is_none();
        (live.on_event)(Event::Preview(preview.unwrap_or_default()));
        if done {
            live.phrase_done();
        }
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

    #[test]
    fn cuts_a_long_phrase_where_its_last_speech_began() {
        let last = LONG_PHRASE / 2..LONG_PHRASE - 100;
        assert_eq!(phrase_end(Some(&last), LONG_PHRASE - 1, 800), None);
        assert_eq!(
            phrase_end(Some(&last), LONG_PHRASE, 800),
            Some(LONG_PHRASE / 2)
        );
        // one long stretch of speech has nowhere to cut
        assert_eq!(
            phrase_end(Some(&(100..LONG_PHRASE)), LONG_PHRASE, 800),
            None
        );
    }
}
