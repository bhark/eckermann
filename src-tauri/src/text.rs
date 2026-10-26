//! Turns the transcribed phrases of a dictation into edits of the text: words are typed,
//! spoken commands become marks or deletions, and a punctuation model adds the marks and
//! capitals nobody said.

use crate::commands::{self, Action, Fit, Item, MARKS};
use crate::punct::Label;

const ENDS: [char; 3] = ['.', '!', '?'];
// typed text kept for deleting; a delete reaches only a few words back
const KEEP_TYPED: usize = 4096;
// words before a phrase that the punctuation model sees
const CONTEXT_WORDS: usize = 48;

/// Delete `delete` characters before the cursor, then type `text`.
#[derive(Debug, PartialEq)]
pub struct Edit {
    pub delete: usize,
    pub text: String,
}

impl Edit {
    pub fn is_empty(&self) -> bool {
        self.delete == 0 && self.text.is_empty()
    }

    /// What the edit does to `doc`, the text before the cursor.
    pub fn apply(&self, doc: &mut String) {
        for _ in 0..self.delete {
            doc.pop();
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
    Spoken(Fit),
}

#[derive(Clone, Default)]
pub struct Joiner {
    tail: Tail,
    // previous phrase without its closing punctuation, as prompt for the next one
    context: String,
    // ponytail: a delete trusts that the cursor stayed where Eckermann typed; no way to check on linux
    typed: String,
    // during a phrase: typed is unchanged before low, and this many characters went from there
    low: usize,
    deleted: usize,
}

impl Joiner {
    pub fn prompt(&self) -> &str {
        &self.context
    }

    /// The edit for the next `phrase` of the dictation; `last` ends the dictation.
    /// `punct` labels words, the ones typed before first; no labels keeps whisper's marks.
    pub fn next(
        &mut self,
        phrase: &str,
        last: bool,
        trailing_space: bool,
        punct: impl FnOnce(&[String]) -> Vec<Label>,
    ) -> Edit {
        self.low = self.typed.len();
        self.deleted = 0;
        let items = commands::parse(phrase);
        let mut words = self.recent_words();
        let before = words.len();
        words.extend(items.iter().filter_map(|item| match item {
            Item::Word(word) if !bare(word).0.is_empty() => Some(bare(word).0.to_string()),
            _ => None,
        }));
        let labels = match words.len() > before {
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
        let mut k = before;
        for (i, item) in items.iter().enumerate() {
            match item {
                Item::Word(word) if bare(word).0.is_empty() => {}
                Item::Word(word) => {
                    self.word(word, label(k), after_end(k), i + 1 == items.len());
                    k += 1;
                }
                Item::Do(Action::Mark(mark, fit)) => self.mark(mark, *fit),
                Item::Do(Action::Delete) => self.delete(),
            }
        }
        let said = phrase.trim_end_matches(ENDS);
        if !said.is_empty() {
            self.context = said.to_string();
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
        let edit = Edit {
            delete: self.deleted,
            text: self.typed[self.low..].to_string(),
        };
        if self.typed.len() > 2 * KEEP_TYPED {
            let from = self.typed.ceil_char_boundary(self.typed.len() - KEEP_TYPED);
            self.typed.drain(..from);
        }
        edit
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
            (own, None) => Tail::Held(own),
            (_, Some(label)) => Tail::Held(label.mark),
        };
    }

    fn mark(&mut self, mark: &'static str, fit: Fit) {
        let spaced = self.typed.trim_end_matches(' ').len();
        let whisper = |c: char| MARKS.contains(&c) || (fit == Fit::Join && c == '-');
        let bare = self.typed[..spaced].trim_end_matches(whisper).len();
        match (&self.tail, fit) {
            (Tail::Spoken(..), _) => self.cut(spaced),
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
        let rest = match typed.trim_end_matches('\n') {
            fewer if fewer.len() < typed.len() => fewer,
            _ => typed
                .trim_end_matches(|c: char| !c.is_alphanumeric() && !c.is_whitespace())
                .trim_end_matches(char::is_alphanumeric),
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
        let deletes = commands::parse(phrase)
            .iter()
            .filter(|item| **item == Item::Do(Action::Delete))
            .count();
        match deletes {
            0 => {
                let edit = self.clone().next(phrase, false, false, punct);
                edit.text.trim_start().to_string()
            }
            n => "⌫".repeat(n),
        }
    }
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
        dictate(&mut Joiner::default(), &mut doc, phrases, punct);
        doc
    }

    /// With a model that only ends the text with a full stop.
    fn typed(phrases: &[&str]) -> String {
        typed_with(phrases, &model(&[], Some('.'), NAMES))
    }

    #[test]
    fn punctuates_and_capitalizes_by_the_model() {
        let m = model(
            &[("tur", ','), ("skoven", '.')],
            Some('.'),
            &["rudolf", "steiner"],
        );
        let mut j = Joiner::default();
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
    fn keeps_whisper_marks_without_a_model() {
        let none = |_: &[String]| Vec::new();
        let mut j = Joiner::default();
        let mut next = |phrase: &str, last: bool| j.next(phrase, last, true, none).text;
        assert_eq!(next("Jeg gik, en tur.", false), "Jeg gik, en tur");
        assert_eq!(next("Og Anne kom", true), ". Og Anne kom ");
    }

    #[test]
    fn prompts_with_the_phrase_before() {
        let none = |_: &[String]| Vec::new();
        let mut j = Joiner::default();
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
        // inside a sentence it is a word
        assert_eq!(
            typed(&["Han kom sent semikolon hun kom slet ikke punktum", ""]),
            "Han kom sent; hun kom slet ikke. "
        );
    }

    #[test]
    fn deletes_into_the_dictation_before() {
        let m = model(&[], Some('.'), NAMES);
        let mut j = Joiner::default();
        let mut doc = String::new();
        dictate(&mut j, &mut doc, &["Jeg har en hund.", ""], &m);
        assert_eq!(doc, "Jeg har en hund. ");
        let slet = j.next("slet", false, true, &m);
        assert_eq!(
            slet,
            Edit {
                delete: 7,
                text: String::new()
            }
        );
        slet.apply(&mut doc);
        dictate(&mut j, &mut doc, &["kat.", ""], &m);
        assert_eq!(doc, "Jeg har en kat. ");
    }

    #[test]
    fn previews_what_will_be_typed() {
        let m = model(&[("kom", ',')], Some('.'), NAMES);
        let mut j = Joiner::default();
        assert_eq!(j.preview("Hej komma du ny linje", &m), "Hej, du\n");
        assert_eq!(j.preview("slet", &m), "⌫");
        // after earlier phrases: their held mark, and no capital inside the sentence
        j.next("Da han kom", false, true, &m);
        assert_eq!(j.preview("Så smilede hun", &m), ", så smilede hun");
        assert_eq!(
            j.next("Så smilede hun", true, false, &m).text,
            ", så smilede hun."
        );
    }
}
