//! Runs whole dictations through the app's path (speech model, commands, punctuation,
//! joining) and compares the text they type with what was meant.
//! cargo run --release --example dictation_eval -- LANGUAGE MODEL.bin VAD.bin PUNCT_DIR CASES.tsv [N]
//! LANGUAGE is da or en; PUNCT_DIR none keeps whisper's own marks.
//! A case is `reference \t audio`: `|` separates phrases, `+` joins clips into one phrase
//! with a pause between them. Paths are relative to CASES.tsv, `\n` in a reference is a line break.
//! STREAM=ms also previews each phrase every ms of its audio, typing what previews agree on.

#[path = "common/wav.rs"]
mod wav;

use eckermann_lib::commands::distance;
use eckermann_lib::lang::Language;
use eckermann_lib::punct::Punctuator;
use eckermann_lib::stt::Stt;
use eckermann_lib::text::Joiner;
use std::path::Path;
use std::time::Instant;

const PAUSE: usize = 16_000 * 7 / 10;

fn words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

/// Words and marks, each mark and line break a token of its own.
fn tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    for c in text.chars() {
        if c.is_alphanumeric() {
            word.extend(c.to_lowercase());
            continue;
        }
        if !word.is_empty() {
            out.push(std::mem::take(&mut word));
        }
        if c == '\n' || !c.is_whitespace() {
            out.push(c.to_string());
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

/// Each word with the mark after it: ',' for , : ; and –, '.' for . ? ! and line breaks.
fn marked(text: &str) -> Vec<(String, char)> {
    let mut out: Vec<(String, char)> = Vec::new();
    for token in tokens(text) {
        let kind = match token.as_str() {
            "," | ":" | ";" | "–" => ',',
            "." | "?" | "!" | "\n" => '.',
            _ if token.chars().all(char::is_alphanumeric) => {
                out.push((token, ' '));
                continue;
            }
            _ => continue,
        };
        if let Some(last) = out.last_mut().filter(|last| last.1 != '.') {
            last.1 = kind;
        }
    }
    out
}

/// Whether each word of `text` starts with a capital, in the order `marked` gives them.
fn capitals(text: &str) -> Vec<bool> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.starts_with(char::is_uppercase))
        .collect()
}

/// Index pairs of the words that a word alignment of `a` and `b` matches.
fn aligned(a: &[(String, char)], b: &[(String, char)]) -> Vec<(usize, usize)> {
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let sub = d[i - 1][j - 1] + usize::from(a[i - 1].0 != b[j - 1].0);
            d[i][j] = sub.min(d[i - 1][j] + 1).min(d[i][j - 1] + 1);
        }
    }
    let (mut i, mut j, mut pairs) = (a.len(), b.len(), Vec::new());
    while i > 0 && j > 0 {
        if a[i - 1].0 == b[j - 1].0 && d[i][j] == d[i - 1][j - 1] {
            pairs.push((i - 1, j - 1));
            (i, j) = (i - 1, j - 1);
        } else if d[i][j] == d[i - 1][j] + 1 {
            i -= 1;
        } else if d[i][j] == d[i][j - 1] + 1 {
            j -= 1;
        } else {
            (i, j) = (i - 1, j - 1);
        }
    }
    pairs
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: dictation_eval LANGUAGE MODEL.bin VAD.bin PUNCT_DIR CASES.tsv [N]";
    let [language, model, vad, punct_dir, cases, rest @ ..] = args.as_slice() else {
        return Err(usage.into());
    };
    let language = match language.as_str() {
        "da" => Language::Da,
        "en" => Language::En,
        _ => return Err(usage.into()),
    };
    let n: usize = rest
        .first()
        .map_or(Ok(usize::MAX), |s| s.parse())
        .map_err(|e| format!("N: {e}"))?;
    let dir = Path::new(cases).parent().unwrap_or(Path::new("."));
    let rows = std::fs::read_to_string(cases).map_err(|e| format!("{cases}: {e}"))?;
    let profile = language.profile();
    let mut stt = Stt::load(Path::new(model), Path::new(vad), profile)?;
    let punctuator = match punct_dir.as_str() {
        "none" => None,
        dir => Some(Punctuator::load(Path::new(dir))?),
    };
    let (mut punct_runs, mut punct_seconds) = (0, 0.0);
    let (mut cases, mut exact, mut lost) = (0, 0, 0);
    let (mut word_errors, mut ref_words, mut mark_errors, mut ref_marks) = (0, 0, 0, 0);
    let (mut phrases, mut seconds) = (0, 0.0);
    let stream = match std::env::var("STREAM") {
        Ok(ms) => Some(ms.parse::<usize>().map_err(|e| format!("STREAM: {e}"))? * 16),
        Err(_) => None,
    };
    let (mut previews, mut preview_seconds, mut early, mut typed_chars) = (0, 0.0, 0, 0);
    let mut corrected = 0;
    // true positives, false positives, misses: commas, sentence ends, capitals
    let mut punct = [[0usize; 3]; 3];
    for row in rows.lines().filter(|l| !l.is_empty()).take(n) {
        let (reference, audio) = row.split_once('\t').ok_or(format!("no tab: {row}"))?;
        let reference = reference.replace("\\n", "\n");
        let mut joiner = Joiner::new(profile);
        let mut doc = String::new();
        let parts: Vec<&str> = audio.split('|').collect();
        let mut heard = Vec::new();
        for (i, part) in parts.iter().enumerate() {
            let mut pcm = Vec::new();
            for (j, clip) in part.split('+').enumerate() {
                if j > 0 {
                    pcm.resize(pcm.len() + PAUSE, 0.0);
                }
                pcm.extend(wav::read_16k_mono(&dir.join(clip).to_string_lossy())?);
            }
            let mut label = |words: &[String]| {
                let Some(punctuator) = &punctuator else {
                    return Vec::new();
                };
                let t = Instant::now();
                let labels = punctuator
                    .label(words)
                    .unwrap_or_else(|e| panic!("punctuate: {e}"));
                punct_seconds += t.elapsed().as_secs_f32();
                punct_runs += 1;
                labels
            };
            for end in stream
                .into_iter()
                .flat_map(|step| (step..pcm.len()).step_by(step))
            {
                let t = Instant::now();
                let text = stt.transcribe(&pcm[..end], joiner.prompt())?;
                preview_seconds += t.elapsed().as_secs_f32();
                previews += 1;
                let (edit, _) = joiner.heard(&text, &mut label);
                early += edit.text.chars().count();
                edit.apply(&mut doc);
            }
            let t = Instant::now();
            let text = stt.transcribe(&pcm, joiner.prompt())?;
            seconds += t.elapsed().as_secs_f32();
            phrases += 1;
            let edit = joiner.next(&text, i + 1 == parts.len(), true, &mut label);
            corrected += edit.delete;
            edit.apply(&mut doc);
            heard.push(text);
        }
        typed_chars += doc.chars().count();
        let typed = doc.trim_end();
        let (want, got) = (words(&reference), words(typed));
        let word_err = distance(&want, &got);
        let marks = tokens(&reference).len() - want.len();
        let mark_err = distance(&tokens(&reference), &tokens(typed)).saturating_sub(word_err);
        let (want_marks, got_marks) = (marked(&reference), marked(typed));
        let (want_caps, got_caps) = (capitals(&reference), capitals(typed));
        let starts = |m: &[(String, char)], i: usize| i == 0 || m[i - 1].1 == '.';
        for (i, j) in aligned(&want_marks, &got_marks) {
            // a capital after a sentence end is grammar; inside a sentence it marks a name
            if !starts(&want_marks, i) && !starts(&got_marks, j) {
                match (want_caps[i], got_caps[j]) {
                    (true, true) => punct[2][0] += 1,
                    (false, true) => punct[2][1] += 1,
                    (true, false) => punct[2][2] += 1,
                    (false, false) => {}
                }
            }
            // the mark after the last word is the end of the text, not punctuation inside it
            if i + 1 == want_marks.len() {
                continue;
            }
            for (k, kind) in [',', '.'].into_iter().enumerate() {
                match (want_marks[i].1 == kind, got_marks[j].1 == kind) {
                    (true, true) => punct[k][0] += 1,
                    (false, true) => punct[k][1] += 1,
                    (true, false) => punct[k][2] += 1,
                    (false, false) => {}
                }
            }
        }
        cases += 1;
        exact += usize::from(typed == reference);
        // under three quarters of the words: a phrase is mostly gone
        lost += usize::from(got.len() * 4 < want.len() * 3);
        word_errors += word_err;
        ref_words += want.len();
        mark_errors += mark_err;
        ref_marks += marks;
        if typed != reference {
            println!("want {reference:?}\n got {typed:?}\nheard {heard:?}");
        }
    }
    println!(
        "cases {cases}, exact {exact}, lost {lost}, WER {:.1}%, mark errors {mark_errors} of {ref_marks}, {:.2}s per phrase",
        100.0 * word_errors as f32 / ref_words.max(1) as f32,
        seconds / phrases.max(1) as f32
    );
    let pct = |a: usize, b: usize| 100.0 * a as f32 / (a + b).max(1) as f32;
    let kinds = ["commas", "sentence ends", "capitals inside sentences"];
    for (name, [tp, fp, miss]) in kinds.iter().zip(punct) {
        println!(
            "{name}: precision {:.0}%, recall {:.0}% ({tp} right, {fp} extra, {miss} missed)",
            pct(tp, fp),
            pct(tp, miss)
        );
    }
    if stream.is_some() {
        println!(
            "previews: {:.2}s each, {:.0}% of the text typed before its pause, {:.0}% of that deleted again",
            preview_seconds / previews.max(1) as f32,
            100.0 * early as f32 / typed_chars.max(1) as f32,
            100.0 * corrected as f32 / early.max(1) as f32
        );
    }
    println!(
        "punctuation: {:.0} ms per run",
        1000.0 * punct_seconds / punct_runs.max(1) as f32
    );
    Ok(())
}
