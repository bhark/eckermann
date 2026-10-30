//! Turns the transcribed phrases of a dictation into edits of the text: words are typed,
//! spoken commands become marks or deletions, and a punctuation model adds the marks and
//! capitals nobody said.

use crate::commands::{self, Action, Fit, Item, MARKS};
use crate::lang::Profile;
use crate::punct::Label;

const ENDS: [char; 3] = ['.', '!', '?'];
// typed text kept for deleting; a delete reaches only a few words back
const KEEP_TYPED: usize = 4096;
// words before a phrase that the punctuation model sees
const CONTEXT_WORDS: usize = 48;

/// Delete `delete` characters before the cursor, then `delete_words` words with the app's
/// own word-delete key, then type `text`.
#[derive(Debug, Default, PartialEq)]
pub struct Edit {
    pub delete: usize,
    /// words from before this dictation, which only the app knows
    pub delete_words: usize,
    pub text: String,
}

impl Edit {
    pub fn is_empty(&self) -> bool {
        self.delete == 0 && self.delete_words == 0 && self.text.is_empty()
    }

    /// What the edit does to `doc`, the text before the cursor.
    pub fn apply(&self, doc: &mut String) {
        for _ in 0..self.delete {
            doc.pop();
        }
        for _ in 0..self.delete_words {
            // ponytail: apps differ on the marks and spaces around a word; this takes them all
            let rest = doc
                .trim_end()
                .trim_end_matches(|c: char| !c.is_whitespace());
            doc.truncate(rest.len());
        }
        doc.push_str(&self.text);
    }
}

/// What the next word follows, which decides its space and capital.
#[derive(Clone, Default)]
enum Tail {
    #[default]
    Start,
    Word,
    /// a word whose natural mark is typed only once the next word comes: a spoken mark
    /// replaces it, and at a phrase's end the next phrase gives the model its right context
    Held(Option<char>),
    /// a word whose held mark a long pause typed; the page may have changed after it
    Settled,
    Spoken(Fit),
}

#[derive(Clone)]
pub struct Joiner {
    profile: &'static Profile,
    tail: Tail,
    // previous phrase without its closing punctuation, as prompt for the next one
    context: String,
    // ponytail: a delete trusts that the cursor stayed where Eckermann typed; no way to check on linux
    typed: String,
    // during a phrase: typed is unchanged before low, and this many characters went from there
    low: usize,
    deleted: usize,
    deleted_words: usize,
    // the phrase being spoken: its items typed already, and its last preview
    spoken: Vec<Item>,
    previewed: Vec<Item>,
    // before the phrase's first early word; its pause types the whole phrase from here again
    before_phrase: Option<Box<Joiner>>,
}

impl Joiner {
    pub fn new(profile: &'static Profile) -> Self {
        Self {
            profile,
            tail: Tail::Start,
            context: String::new(),
            typed: String::new(),
            low: 0,
            deleted: 0,
            deleted_words: 0,
            spoken: Vec::new(),
            previewed: Vec::new(),
            before_phrase: None,
        }
    }

    pub fn prompt(&self) -> &str {
        &self.context
    }

    /// The edit for the next `phrase` of the dictation; `last` ends the dictation. Words
    /// `heard` typed early are corrected where the whole phrase heard them differently.
    /// `punct` labels words, the ones typed before first; no labels keeps whisper's marks.
    pub fn next(
        &mut self,
        phrase: &str,
        last: bool,
        trailing_space: bool,
        punct: impl FnOnce(&[String]) -> Vec<Label>,
    ) -> Edit {
        let shown = self
            .before_phrase
            .take()
            .map(|before| std::mem::replace(self, *before).typed);
        self.spoken.clear();
        self.previewed.clear();
        let said = phrase.trim_end_matches(ENDS);
        if !said.is_empty() {
            self.context = said.to_string();
        }
        let items = commands::parse(phrase, self.profile);
        let mut edit = self.type_items(&items, &[], last, trailing_space, punct);
        if let Some(shown) = shown {
            // a word deleted from before the dictation goes after all the phrase typed
            let same = match edit.delete_words {
                0 => shown
                    .chars()
                    .zip(self.typed.chars())
                    .take_while(|(a, b)| a == b)
                    .map(|(c, _)| c.len_utf8())
                    .sum(),
                _ => 0,
            };
            edit.delete = shown[same..].chars().count();
            edit.text = self.typed[same..].to_string();
        }
        // only between phrases, so early words and their phrase line up in typed
        if self.typed.len() > 2 * KEEP_TYPED {
            let from = self.typed.ceil_char_boundary(self.typed.len() - KEEP_TYPED);
            self.typed.drain(..from);
        }
        edit
    }

    /// Types the held mark, once the pause is long enough that the user may edit next.
    pub fn settle(&mut self) -> Edit {
        self.low = self.typed.len();
        if let Tail::Held(mark) = self.tail {
            self.typed.extend(mark);
            self.tail = Tail::Settled;
        }
        Edit {
            text: self.typed[self.low..].to_string(),
            ..Edit::default()
        }
    }

    /// Types the start of the phrase being spoken once two previews in a row agree on it.
    /// Returns that edit, and what the rest of `phrase` would type now.
    pub fn heard(
        &mut self,
        phrase: &str,
        mut punct: impl FnMut(&[String]) -> Vec<Label>,
    ) -> (Edit, String) {
        if self.spoken.is_empty() && !commands::is_text(phrase, self.profile) {
            return (Edit::default(), self.preview(phrase, punct));
        }
        let items = commands::parse_text(phrase, self.profile);
        let from = typed_until(&self.spoken, &items);
        let before = &self.previewed[typed_until(&self.spoken, &self.previewed)..];
        let agreed = items[from..]
            .iter()
            .zip(before)
            .take_while(|(a, b)| key(a) == key(b))
            .count();
        // the last may still grow into a command; holding two cut own recordings' WER from 4.8% to 3.8%
        let sure = &items[from..from + agreed.saturating_sub(2)];
        let rest = &items[from + sure.len()..];
        if !sure.is_empty() && self.before_phrase.is_none() {
            self.before_phrase = Some(Box::new(self.clone()));
        }
        let edit = self.type_items(sure, rest, false, false, &mut punct);
        self.spoken.extend_from_slice(sure);
        let preview = self.clone().type_items(rest, &[], false, false, punct).text;
        self.previewed = items;
        (edit, preview.trim_start_matches(' ').to_string())
    }

    /// `ahead`: items still to come, which the model sees but are not typed.
    fn type_items(
        &mut self,
        items: &[Item],
        ahead: &[Item],
        last: bool,
        trailing_space: bool,
        punct: impl FnOnce(&[String]) -> Vec<Label>,
    ) -> Edit {
        self.low = self.typed.len();
        self.deleted = 0;
        self.deleted_words = 0;
        let bare_words = |items: &[Item]| -> Vec<String> {
            items
                .iter()
                .filter_map(|item| match item {
                    Item::Word(word) if !bare(word).0.is_empty() => Some(bare(word).0.to_string()),
                    _ => None,
                })
                .collect()
        };
        let mut words = self.recent_words();
        let before = words.len();
        words.extend(bare_words(items));
        let typing = words.len() > before;
        words.extend(bare_words(ahead));
        let labels = match typing {
            true => punct(&words),
            false => Vec::new(),
        };
        let label = |i: usize| labels.get(i).copied();
        if let (Tail::Held(mark), Some(held)) = (&mut self.tail, before.checked_sub(1)) {
            // the held word is the last one typed; now the model sees what follows it
            if let Some(l) = label(held) {
                *mark = l.mark;
            }
        }
        // the model capitalizes its first word and after its sentence ends, which may not be typed
        let after_end = |i: usize| i == 0 || label(i - 1).is_some_and(|l| l.ends_sentence);
        let whisper_marks = self.profile.whisper_marks;
        let mut k = before;
        for (i, item) in items.iter().enumerate() {
            match item {
                Item::Word(word) if bare(word).0.is_empty() => {}
                Item::Word(word) => {
                    let l = label(k).filter(|_| !whisper_marks || k == before);
                    self.word(word, l, after_end(k), i + 1 == items.len());
                    k += 1;
                }
                Item::Do(Action::Mark(mark, fit)) => self.mark(mark, *fit),
                Item::Do(Action::Delete) => self.delete(),
            }
        }
        if last {
            if let Tail::Held(Some(mark)) = self.tail {
                self.typed.push(mark);
            }
            let open = !matches!(
                self.tail,
                Tail::Start | Tail::Spoken(Fit::Line | Fit::Right | Fit::Join)
            );
            if trailing_space && open {
                self.typed.push(' ');
            }
            self.tail = Tail::Start;
            self.context.clear();
        }
        Edit {
            delete: self.deleted,
            delete_words: self.deleted_words,
            text: self.typed[self.low..].to_string(),
        }
    }

    /// The last words typed, without their marks.
    fn recent_words(&self) -> Vec<String> {
        let mut words: Vec<String> = self
            .typed
            .split_whitespace()
            .rev()
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
            .filter(|w| !w.is_empty())
            .take(CONTEXT_WORDS)
            .map(String::from)
            .collect();
        words.reverse();
        words
    }

    /// `after_end`: the model saw a sentence start before the word, which may not be typed.
    fn word(&mut self, word: &str, label: Option<Label>, after_end: bool, ends_phrase: bool) {
        let (word, own) = bare(word);
        if let Tail::Held(Some(mark)) = self.tail {
            self.typed.push(mark);
        }
        let before = self.typed.trim_end_matches(' ');
        let fresh = before.is_empty() || before.ends_with(ENDS) || before.ends_with('\n');
        let word = match (fresh, label) {
            (true, _) => upper_first(word),
            (false, Some(l)) if l.capital && !after_end => upper_first(word),
            (false, Some(_)) => lower_first(word),
            (false, None) => word.to_string(),
        };
        let spaced = !matches!(
            self.tail,
            Tail::Start | Tail::Spoken(Fit::Line | Fit::Right | Fit::Join)
        );
        if spaced {
            self.typed.push(' ');
        }
        self.typed.push_str(&word);
        // inside a phrase, edda's own marks are mostly spoken ones; at its end, edda ends every clip like a sentence
        self.tail = match (own.chars().next(), label) {
            (Some(_), _) if !ends_phrase => {
                self.typed.push_str(own);
                Tail::Word
            }
            (_, Some(label)) if !self.profile.whisper_marks => Tail::Held(label.mark),
            (own, _) => Tail::Held(own),
        };
    }

    fn mark(&mut self, mark: &'static str, fit: Fit) {
        let spaced = self.typed.trim_end_matches(' ').len();
        let whisper = |c: char| MARKS.contains(&c) || (fit == Fit::Join && c == '-');
        let bare = self.typed[..spaced].trim_end_matches(whisper).len();
        match (&self.tail, fit) {
            (Tail::Spoken(..) | Tail::Settled, _) => self.cut(spaced),
            // a line break keeps the sentence end whisper wrote, not its commas
            (_, Fit::Line) => {
                let end = self.typed[..spaced].trim_end_matches([',', ':', ';']).len();
                self.cut(end);
            }
            // a spoken mark replaces whisper's own punctuation next to it
            _ => self.cut(bare),
        }
        let fit = match fit {
            Fit::Quote if self.typed.matches('"').count().is_multiple_of(2) => Fit::Right,
            Fit::Quote => Fit::Left,
            fit => fit,
        };
        let spaced = matches!(fit, Fit::Right | Fit::Apart)
            && !self.typed.is_empty()
            && !self.typed.ends_with('\n');
        if spaced {
            self.typed.push(' ');
        }
        self.typed.push_str(mark);
        self.tail = Tail::Spoken(fit);
    }

    /// Removes the previous word with the marks after it, or a line break.
    fn delete(&mut self) {
        let typed = self.typed.trim_end_matches(' ');
        if typed.is_empty() {
            self.cut(0);
            self.deleted_words += 1;
            self.tail = Tail::Start;
            return;
        }
        let rest = match typed.trim_end_matches('\n') {
            fewer if fewer.len() < typed.len() => fewer,
            // to the space before, so "e-mail" or "14.30" goes whole
            _ => typed.trim_end_matches(|c: char| !c.is_whitespace()),
        };
        self.cut(rest.trim_end_matches(' ').len());
        self.tail = match self.typed.chars().last() {
            None => Tail::Start,
            Some('\n') => Tail::Spoken(Fit::Line),
            Some(c) if ENDS.contains(&c) => Tail::Spoken(Fit::Left),
            Some(_) => Tail::Word,
        };
    }

    /// Shortens the typed text to `len` bytes, counting what was typed before this phrase.
    fn cut(&mut self, len: usize) {
        if len < self.low {
            self.deleted += self.typed[len..self.low].chars().count();
            self.low = len;
        }
        self.typed.truncate(len);
    }

    /// The next `phrase` as it would be typed now, for the overlay; a delete shows as ⌫.
    pub fn preview(&self, phrase: &str, punct: impl FnOnce(&[String]) -> Vec<Label>) -> String {
        let deletes = commands::parse(phrase, self.profile)
            .iter()
            .filter(|item| **item == Item::Do(Action::Delete))
            .count();
        match deletes {
            0 => {
                let edit = self.clone().next(phrase, false, false, punct);
                edit.text.trim_start_matches(' ').to_string()
            }
            n => "⌫".repeat(n),
        }
    }
}

/// The item without whisper's case and marks, to compare two readings of a phrase.
fn key(item: &Item) -> Item {
    match item {
        Item::Word(word) => Item::Word(commands::core(word)),
        Item::Do(action) => Item::Do(*action),
    }
}

/// Where the typed `spoken` items end in `items`, a new reading of their phrase.
fn typed_until(spoken: &[Item], items: &[Item]) -> usize {
    let keys = |items: &[Item]| items.iter().map(key).collect::<Vec<_>>();
    let row = commands::distances(&keys(spoken), &keys(items));
    // on a tie the furthest, as the final reading mostly adds marks the previews lacked
    (0..row.len())
        .min_by_key(|&j| (row[j], std::cmp::Reverse(j)))
        .unwrap_or(0)
}

/// The word without whisper's marks after it, and those marks.
fn bare(word: &str) -> (&str, &str) {
    word.split_at(word.trim_end_matches(MARKS).len())
}

/// Lowercases the first letter unless the word is an acronym.
fn lower_first(text: &str) -> String {
    let mut chars = text.chars();
    match (chars.next(), chars.clone().next()) {
        (Some(first), second) if !second.is_some_and(char::is_uppercase) => {
            first.to_lowercase().chain(chars).collect()
        }
        _ => text.to_string(),
    }
}

fn upper_first(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::Language;

    fn joiner() -> Joiner {
        Joiner::new(Language::Da.profile())
    }

    /// A stand-in for the punctuation model: marks after the words in `marks`, else `end`
    /// after the last word it sees, and capitals for `names`, its first word and after its sentence ends.
    fn model(
        marks: &'static [(&'static str, char)],
        end: Option<char>,
        names: &'static [&'static str],
    ) -> impl Fn(&[String]) -> Vec<Label> {
        move |words| {
            let mut labels: Vec<Label> = Vec::new();
            for (i, w) in words.iter().enumerate() {
                let w = w.to_lowercase();
                let mark = marks.iter().find(|m| m.0 == w).map(|m| m.1);
                let mark = mark.or(end.filter(|_| i + 1 == words.len()));
                let after_end = labels.last().is_none_or(|l| l.ends_sentence);
                labels.push(Label {
                    mark,
                    capital: names.contains(&w.as_str()) || after_end,
                    ends_sentence: mark.is_some_and(|m| ENDS.contains(&m)),
                });
            }
            labels
        }
    }

    const NAMES: &[&str] = &["anne", "jens", "aarhus"];

    /// The text one dictation types; the last phrase ends it.
    fn dictate(
        j: &mut Joiner,
        doc: &mut String,
        phrases: &[&str],
        punct: &impl Fn(&[String]) -> Vec<Label>,
    ) {
        for (i, phrase) in phrases.iter().enumerate() {
            j.next(phrase, i + 1 == phrases.len(), true, punct)
                .apply(doc);
        }
    }

    fn typed_with(phrases: &[&str], punct: &impl Fn(&[String]) -> Vec<Label>) -> String {
        let mut doc = String::new();
        dictate(&mut joiner(), &mut doc, phrases, punct);
        doc
    }

    /// With a model that only ends the text with a full stop.
    fn typed(phrases: &[&str]) -> String {
        typed_with(phrases, &model(&[], Some('.'), NAMES))
    }

    /// As `typed`, dictating English; the model capitalizes "I" as a name.
    fn typed_en(phrases: &[&str]) -> String {
        let mut doc = String::new();
        let m = model(&[], Some('.'), &["anne", "i"]);
        dictate(
            &mut Joiner::new(Language::En.profile()),
            &mut doc,
            phrases,
            &m,
        );
        doc
    }

    #[test]
    fn keeps_whisper_marks_inside_english_phrases() {
        let mut doc = String::new();
        // the model would drop the comma after "York" and lowercase it
        let m = model(&[("think", '.'), ("walk", ',')], Some('.'), &[]);
        let phrases = [
            "It is in New York, I think.",
            "I went for a walk.",
            "And then home.",
            "",
        ];
        dictate(
            &mut Joiner::new(Language::En.profile()),
            &mut doc,
            &phrases,
            &m,
        );
        assert_eq!(
            doc,
            "It is in New York, I think. I went for a walk, and then home. "
        );
    }

    #[test]
    fn types_spoken_marks_in_english() {
        let cases = [
            (
                "I went home period then I ate comma and slept.",
                "I went home. Then I ate, and slept. ",
            ),
            (
                "The book open parenthesis which came out this year close parenthesis was a success period",
                "The book (which came out this year) was a success. ",
            ),
            (
                "It is a black hyphen white film period",
                "It is a black-white film. ",
            ),
            ("He came em dash but too late.", "He came—but too late. "),
            (
                "He said open quote I am coming tomorrow close quote period",
                "He said \"I am coming tomorrow\". ",
            ),
            ("Choose yes slash no full stop", "Choose yes/no. "),
            ("Is it true question mark", "Is it true? "),
            ("It costs $20 period", "It costs $20. "),
            ("first point new line", "First point\n"),
        ];
        for (heard, want) in cases {
            assert_eq!(typed_en(&[heard, ""]), want, "{heard}");
        }
        assert_eq!(
            typed_en(&["Dear Anne comma new line thank you for the letter.", ""]),
            "Dear Anne,\nThank you for the letter. "
        );
        assert_eq!(
            typed_en(&["I have a dog.", "Delete that.", "cat period", ""]),
            "I have a cat. "
        );
        // inside a sentence it is a word
        assert_eq!(
            typed_en(&["Please delete the file period", ""]),
            "Please delete the file. "
        );
    }

    #[test]
    fn punctuates_and_capitalizes_by_the_model() {
        let m = model(
            &[("tur", ','), ("skoven", '.')],
            Some('.'),
            &["rudolf", "steiner"],
        );
        let mut j = joiner();
        let mut next = |phrase: &str, last: bool| j.next(phrase, last, true, &m).text;
        // a phrase's last mark waits for the next phrase, which ends its guess
        assert_eq!(next("Jeg gik en tur.", false), "Jeg gik en tur");
        assert_eq!(next("I skoven", false), ", i skoven");
        // edda's capital at the start of a clip gives way, a name keeps one
        assert_eq!(
            next("Og Rudolf steiner kom", false),
            ". Og Rudolf Steiner kom"
        );
        assert_eq!(next("DR sendte det", false), " DR sendte det");
        // a spoken mark replaces the model's sentence end, and the capital after it goes too
        assert_eq!(next("skoven komma Og så", false), " skoven, og så");
        // the dictation's end types the guess
        assert_eq!(next("", true), ". ");
        assert_eq!(next("hej", true), "Hej. ");
    }

    #[test]
    fn settles_the_held_mark_for_good() {
        let m = model(&[("tur", ',')], Some('.'), &[]);
        let mut j = joiner();
        let mut next = |phrase: &str| j.next(phrase, false, true, &m).text;
        assert_eq!(next("Jeg gik en tur."), "Jeg gik en tur");
        assert_eq!(j.settle().text, ",");
        assert_eq!(j.settle().text, "");
        // a spoken line break no longer takes the comma back
        assert_eq!(
            j.next("ny linje i skoven", false, true, &m).text,
            "\nI skoven"
        );
    }

    #[test]
    fn keeps_whisper_marks_without_a_model() {
        let none = |_: &[String]| Vec::new();
        let mut j = joiner();
        let mut next = |phrase: &str, last: bool| j.next(phrase, last, true, none).text;
        assert_eq!(next("Jeg gik, en tur.", false), "Jeg gik, en tur");
        assert_eq!(next("Og Anne kom", true), ". Og Anne kom ");
    }

    #[test]
    fn prompts_with_the_phrase_before() {
        let none = |_: &[String]| Vec::new();
        let mut j = joiner();
        j.next("Jeg gik en tur.", false, true, none);
        assert_eq!(j.prompt(), "Jeg gik en tur");
        j.next("", true, true, none);
        assert_eq!(j.prompt(), "");
    }

    #[test]
    fn types_spoken_marks() {
        let cases = [
            (
                "Jeg gik hjem punktum så spiste jeg komma og sov.",
                "Jeg gik hjem. Så spiste jeg, og sov. ",
            ),
            // whisper's own marks next to a spoken one give way
            (
                "Hvad nu. Spørgsmålstegn. Intet, kolon, slut.",
                "Hvad nu? Intet: slut. ",
            ),
            (
                "Bogen start parentes som udkom i år slut parentes blev en succes punktum",
                "Bogen (som udkom i år) blev en succes. ",
            ),
            (
                "Det er en sort bindestreg hvid film punktum",
                "Det er en sort-hvid film. ",
            ),
            (
                "Han kom tankestreg men for sent.",
                "Han kom – men for sent. ",
            ),
            (
                "Han sagde citationstegn jeg kommer i morgen citationstegn punktum",
                "Han sagde \"jeg kommer i morgen\". ",
            ),
            ("Vælg ja skråstreg nej punktum", "Vælg ja/nej. "),
            // the model's capital for the first word it sees is no name
            (
                "start parentes Som udkom i år slut parentes",
                "(som udkom i år) ",
            ),
            ("Valg ja- skråstreg nejpunktum", "Valg ja/nejpunktum. "),
            // the model's mark gives way to a spoken line break
            ("første punkt ny linje", "Første punkt\n"),
            ("Det er Jens apostrof bil.", "Det er Jens' bil. "),
            // a spoken mark stays before the next one
            (
                "Han spurgte citationstegn hvor er du spørgsmålstegn citationstegn",
                "Han spurgte \"hvor er du?\" ",
            ),
        ];
        for (heard, want) in cases {
            assert_eq!(typed(&[heard, ""]), want, "{heard}");
        }
    }

    #[test]
    fn breaks_lines_across_phrases() {
        assert_eq!(
            typed(&[
                "Kapitel et ny linje",
                "det var nat.",
                "Nyt afsnit",
                "Næste dag linjeskift",
                "",
            ]),
            "Kapitel et\nDet var nat\n\nNæste dag\n"
        );
        assert_eq!(
            typed(&["Kære Anne komma linieskift tak for brevet.", ""]),
            "Kære Anne,\nTak for brevet. "
        );
        // whisper's commas around a spoken line break go
        assert_eq!(
            typed(&[
                "Første punkt, nye linje, andet punkt, ny linje, tredje punkt",
                ""
            ]),
            "Første punkt\nAndet punkt\nTredje punkt. "
        );
    }

    #[test]
    fn copes_with_how_edda_writes_spoken_marks() {
        let cases: [(&[&str], &str); 5] = [
            (
                &[
                    "Det var en mørk dag i april, , hvor vinden",
                    "De piskede i træerne.",
                    "punktum",
                    "",
                ],
                "Det var en mørk dag i april, hvor vinden de piskede i træerne. ",
            ),
            (
                &[
                    "Jeg gik mig over sø og land,”Komma der mødte jeg en gammel mand§",
                    "",
                ],
                "Jeg gik mig over sø og land, der mødte jeg en gammel mand. ",
            ),
            (
                &[
                    "Jeg gik mig over sø og land,\",Komma der mødte jeg en gammel mand",
                    "",
                ],
                "Jeg gik mig over sø og land, der mødte jeg en gammel mand. ",
            ),
            // said "komma" at a pause
            (
                &[
                    "Jeg gik,.\\Punktum",
                    "Vinden piskede i træerne,.\\Punktum",
                    "og sang",
                    "",
                ],
                "Jeg gik, vinden piskede i træerne, og sang. ",
            ),
            (
                &["Jeg gik en tur.\\PunktumNy linje", "Hvorfor?", ""],
                "Jeg gik en tur.\nHvorfor? ",
            ),
        ];
        let m = model(&[("hvorfor", '?')], Some('.'), NAMES);
        for (heard, want) in cases {
            assert_eq!(typed_with(heard, &m), want, "{heard:?}");
        }
    }

    #[test]
    fn deletes_the_previous_word() {
        assert_eq!(
            typed(&["Jeg har en hund.", "Slet.", "kat punktum", ""]),
            "Jeg har en kat. "
        );
        assert_eq!(
            typed(&[
                "Vi tog toget til Aarhus i går.",
                "slet",
                "slet",
                "i forgårs punktum",
                ""
            ]),
            "Vi tog toget til Aarhus i forgårs. "
        );
        assert_eq!(typed(&["Først ny linje", "slet", "så", ""]), "Først så. ");
        assert_eq!(typed(&["Hej punktum", "slet slet", "Davs", ""]), "Davs. ");
        assert_eq!(typed(&["slet", ""]), "");
        // a word with marks inside goes whole
        assert_eq!(
            typed(&["Send en e-mail", "slet", "besked punktum", ""]),
            "Send en besked. "
        );
        assert_eq!(
            typed(&["Mødet er kl. 14.30", "slet", "15 punktum", ""]),
            "Mødet er kl. 15. "
        );
        // inside a sentence it is a word
        assert_eq!(
            typed(&["Han kom sent semikolon hun kom slet ikke punktum", ""]),
            "Han kom sent; hun kom slet ikke. "
        );
    }

    #[test]
    fn deletes_text_from_before_the_dictation_by_word() {
        let m = model(&[], Some('.'), NAMES);
        let mut j = joiner();
        let mut doc = String::from("Jeg har en hund. ");
        let slet = j.next("slet", false, true, &m);
        assert_eq!(
            slet,
            Edit {
                delete: 0,
                delete_words: 1,
                text: String::new()
            }
        );
        slet.apply(&mut doc);
        dictate(&mut j, &mut doc, &["kat.", ""], &m);
        // what comes before the cursor is unknown, so the word starts a sentence
        assert_eq!(doc, "Jeg har en Kat. ");
    }

    #[test]
    fn previews_what_will_be_typed() {
        let m = model(&[("kom", ',')], Some('.'), NAMES);
        let mut j = joiner();
        assert_eq!(j.preview("Hej komma du ny linje", &m), "Hej, du\n");
        assert_eq!(j.preview("slet", &m), "⌫");
        assert_eq!(j.preview("Linjeskift.", &m), "\n");
        // after earlier phrases: their held mark, and no capital inside the sentence
        j.next("Da han kom", false, true, &m);
        assert_eq!(j.preview("Så smilede hun", &m), ", så smilede hun");
        assert_eq!(
            j.next("Så smilede hun", true, false, &m).text,
            ", så smilede hun."
        );
    }

    /// As `typed_with`, but each phrase also comes word by word first, as previews do.
    fn streamed(phrases: &[&str], punct: &impl Fn(&[String]) -> Vec<Label>) -> String {
        let mut j = joiner();
        let mut doc = String::new();
        for (i, phrase) in phrases.iter().enumerate() {
            let words: Vec<&str> = phrase.split_whitespace().collect();
            for n in 1..words.len() {
                // edda ends each clip like a sentence
                let (edit, _) = j.heard(&format!("{}.", words[..n].join(" ")), punct);
                edit.apply(&mut doc);
            }
            j.next(phrase, i + 1 == phrases.len(), true, punct)
                .apply(&mut doc);
        }
        doc
    }

    #[test]
    fn types_agreed_words_as_the_whole_phrase_would() {
        let m = model(&[("tur", ','), ("skoven", '.')], Some('.'), NAMES);
        let cases: [&[&str]; 5] = [
            &["Jeg gik en tur i skoven og så en hjort", ""],
            &["Vi skriver her ny linje og så videre", ""],
            &["Hun kom slet ikke til festen i går", ""],
            &["Jeg gik hjem komma og så sov jeg punktum", ""],
            &[
                "Jeg gik en tur i skoven.",
                "Og så kom Anne hjem igen",
                "slet",
                "",
            ],
        ];
        for phrases in cases {
            assert_eq!(
                streamed(phrases, &m),
                typed_with(phrases, &m),
                "{phrases:?}"
            );
        }
    }

    #[test]
    fn waits_for_a_command_to_be_whole() {
        let m = model(&[], Some('.'), NAMES);
        let mut j = joiner();
        assert_eq!(j.heard("vi skriver her ny", &m).0, Edit::default());
        let (edit, rest) = j.heard("vi skriver her ny linje", &m);
        assert_eq!(edit.text, "Vi");
        assert_eq!(rest, "skriver her\n");
        // alone, it may be a command until the pause
        let mut j = joiner();
        j.heard("slet slet", &m);
        assert_eq!(j.heard("slet slet", &m), (Edit::default(), "⌫⌫".into()));
    }

    #[test]
    fn corrects_early_words_the_whole_phrase_heard_differently() {
        let m = model(&[], Some('.'), NAMES);
        let mut j = joiner();
        let mut doc = String::new();
        j.heard("Jeg gik en tur.", &m).0.apply(&mut doc);
        j.heard("Jeg gik en tur i.", &m).0.apply(&mut doc);
        j.heard("Jeg gik en tur i skoven.", &m).0.apply(&mut doc);
        assert_eq!(doc, "Jeg gik en");
        let edit = j.next("Jeg gik den tur i skoven.", true, true, &m);
        assert_eq!(edit.delete, 2);
        edit.apply(&mut doc);
        assert_eq!(doc, "Jeg gik den tur i skoven. ");
        // edda's spoken marks come in late, but the words typed stay typed once
        let da = Language::Da.profile();
        let typed = commands::parse_text("Du skal købe følgende mælk brød", da);
        let fin =
            commands::parse_text("Du skal købe følgende,’Kolon mælk,’Komma brød og smør.", da);
        assert_eq!(typed_until(&typed, &fin), 8);
    }
}
