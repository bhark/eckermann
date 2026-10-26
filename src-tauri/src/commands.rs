//! Spoken commands: words that type a mark or edit the text instead of themselves.
//! The phrasings come from the language's profile.

use crate::lang::Profile;
use Action::{Delete, Mark};

/// The marks whisper itself writes after words.
pub const MARKS: [char; 6] = ['.', ',', '!', '?', ':', ';'];

/// How a mark sits between the words around it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fit {
    /// against the word before
    Left,
    /// against the word after
    Right,
    /// against both words
    Join,
    /// spaced from both words
    Apart,
    /// ends the line, so the next word starts a new one
    Line,
    /// opens or closes, by how many came before
    Quote,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    Mark(&'static str, Fit),
    /// removes the previous word and its marks, or a line break
    Delete,
}

/// A command's phrasings, in lowercase, and what it does.
pub type Command = (&'static [&'static str], Action);

#[derive(Debug, PartialEq)]
pub enum Item {
    Word(String),
    Do(Action),
}

/// Splits a transcribed phrase into words and commands. Editing commands count only
/// in a phrase of commands alone, so "hun kom slet ikke" stays text.
pub fn parse(phrase: &str, profile: &Profile) -> Vec<Item> {
    let text = plain(phrase, profile);
    let words: Vec<&str> = text.split_whitespace().collect();
    let commands = profile.commands;
    let all = scan(&words, true, commands);
    if all.iter().all(|item| matches!(item, Item::Do(_))) {
        return all;
    }
    let heard_one = all.iter().any(|item| matches!(item, Item::Do(_)));
    if let Some(action) = misheard(&words, commands).filter(|_| !heard_one) {
        return vec![Item::Do(action)];
    }
    scan(&words, false, commands)
}

fn scan(words: &[&str], editing: bool, commands: &'static [Command]) -> Vec<Item> {
    let mut items = Vec::new();
    let mut rest = words;
    while let Some(first) = rest.first() {
        match command_at(rest, commands).filter(|&(_, action)| editing || action != Delete) {
            Some((len, action)) => {
                items.push(Item::Do(action));
                rest = &rest[len..];
            }
            None => {
                items.push(Item::Word(first.to_string()));
                rest = &rest[1..];
            }
        }
    }
    items
}

pub fn phrasings(commands: &'static [Command]) -> impl Iterator<Item = (&'static str, Action)> {
    commands
        .iter()
        .flat_map(|(said, action)| said.iter().map(move |s| (*s, *action)))
}

/// The command that `words` start with, and how many words it took; the longest wins.
fn command_at(words: &[&str], commands: &'static [Command]) -> Option<(usize, Action)> {
    // a lone sign is edda writing the spoken word as its mark
    if let Some(sign) = words
        .first()
        .filter(|w| w.len() == 1 && w.starts_with(MARKS))
    {
        return phrasings(commands)
            .find(|(_, action)| matches!(action, Mark(m, Fit::Left) if m == sign))
            .map(|(_, action)| (1, action));
    }
    phrasings(commands)
        .filter_map(|(said, action)| {
            let n = said.split(' ').count();
            let hit = n <= words.len() && said.split(' ').zip(words).all(|(s, w)| core(w) == s);
            hit.then_some((n, action))
        })
        .max_by_key(|&(n, _)| n)
        .or_else(|| {
            // long names are distinctive, so a word close to one is that command misheard
            let heard: Vec<char> = core(words[0]).chars().collect();
            phrasings(commands)
                .filter(|(said, _)| !said.contains(' '))
                .find(|(said, _)| {
                    let said: Vec<char> = said.chars().collect();
                    said.len() >= 9 && distance(&heard, &said) <= said.len() / 5
                })
                .map(|(_, action)| (1, action))
        })
}

/// A short phrase said alone may be a misheard command, such as "nye linjer".
fn misheard(words: &[&str], commands: &'static [Command]) -> Option<Action> {
    if words.is_empty() || words.len() > 3 {
        return None;
    }
    let heard: Vec<char> = words
        .iter()
        .flat_map(|w| core(w).chars().collect::<Vec<_>>())
        .collect();
    phrasings(commands)
        .filter_map(|(said, action)| {
            let said: Vec<char> = said.chars().filter(|&c| c != ' ').collect();
            // short names are too close to everyday words, such as komma and komme
            let allowed = match said.len() {
                ..7 => return None,
                n => n / 4,
            };
            let d = distance(&heard, &said);
            (d <= allowed).then_some((d, action))
        })
        .min_by_key(|&(d, _)| d)
        .map(|(_, action)| action)
}

fn core(word: &str) -> String {
    word.trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase()
}

/// Levenshtein distance between two sequences.
pub fn distance<T: PartialEq>(a: &[T], b: &[T]) -> usize {
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, x) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, y) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(x != y)).min(row[j] + 1).min(cur + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

/// The phrase's words, edda's tags repaired first, with each split compound joined again.
fn plain(phrase: &str, profile: &Profile) -> String {
    let repaired = profile
        .nst_tags
        .then(|| crate::edda::plain(phrase, profile.commands));
    let words: Vec<&str> = repaired
        .as_deref()
        .unwrap_or(phrase)
        .split_whitespace()
        .collect();
    let mut text = String::new();
    for (i, word) in words.iter().enumerate() {
        text += word;
        // "sort- hvid" is a spoken bindestreg; a split compound goes on with a conjunction
        let compound = words.get(i + 1).is_some_and(|next| {
            let next = core(next);
            let command = phrasings(profile.commands)
                .any(|(said, _)| said.split(' ').next() == Some(next.as_str()));
            !command && !profile.compound_joins.contains(&next.as_str())
        });
        if !(compound && word.len() > 1 && word.ends_with('-')) {
            text.push(' ');
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::Language;
    use Fit::{Line, Right};

    fn parse(phrase: &str) -> Vec<Item> {
        super::parse(phrase, Language::Da.profile())
    }

    fn phrasings() -> impl Iterator<Item = (&'static str, Action)> {
        super::phrasings(Language::Da.profile().commands)
    }

    fn words(items: &[Item]) -> Vec<String> {
        items
            .iter()
            .map(|item| match item {
                Item::Word(w) => w.clone(),
                Item::Do(Mark(m, _)) => format!("<{m}>"),
                Item::Do(Delete) => "<slet>".into(),
            })
            .collect()
    }

    #[test]
    fn makes_edda_tags_plain() {
        let cases = [
            ("land,”Komma der mand§", "land <,> der mand"),
            ("land,\",Komma der", "land <,> der"),
            ("land,\", der mand.", "land, der mand."),
            ("Jeg gik,.\\Punktum", "Jeg gik <,>"),
            ("en tur.\\PunktumNy linje", "en tur <.> <\n>"),
            ("Hej.Komma du", "Hej <,> du"),
            ("Vent:Kolon nu", "Vent <:> nu"),
            ("april, , hvor", "april, <,> hvor"),
            ("Hvad hedder du?”Spørgsmålstegn", "Hvad hedder du <?>"),
            ("følgende,mælk,brød og 3,5", "følgende, mælk, brød og 3,5"),
            ("Første punkt, nye linje, andet", "Første punkt, <\n> andet"),
            (
                "sort- hvid, bevarelses- og adgangs",
                "sort-hvid, bevarelses- og adgangs",
            ),
            ("ja- skråstreg nej", "ja- </> nej"),
            (
                "følgende,’Kolon mælk,’Komma brød",
                "følgende <:> mælk <,> brød",
            ),
            ("sort-Bindestreg hvid", "sort- <-> hvid"),
            ("Kære Anne,comma", "Kære Anne <,>"),
            ("citerationstegn hvor", "<\"> hvor"),
        ];
        for (heard, want) in cases {
            assert_eq!(words(&parse(heard)).join(" "), want, "{heard}");
        }
    }

    #[test]
    fn knows_every_phrasing() {
        for (said, action) in phrasings() {
            let want = match action {
                Delete => Item::Word(said.into()),
                _ => Item::Do(action),
            };
            assert_eq!(parse(&format!("a {said} b")).get(1), Some(&want), "{said}");
        }
        assert_eq!(parse("Linjeskift."), vec![Item::Do(Mark("\n", Line))]);
        assert_eq!(parse("Start parentes"), vec![Item::Do(Mark("(", Right))]);
    }

    #[test]
    fn edits_only_when_said_alone() {
        assert_eq!(parse("Slet."), vec![Item::Do(Delete)]);
        assert_eq!(parse("slet slet"), vec![Item::Do(Delete), Item::Do(Delete)]);
        assert_eq!(
            words(&parse("hun kom slet ikke")).join(" "),
            "hun kom slet ikke"
        );
        assert_eq!(
            words(&parse("Jeg vil slette filen.")).join(" "),
            "Jeg vil slette filen."
        );
    }

    #[test]
    fn accepts_misheard_commands_said_alone() {
        assert_eq!(parse("ny linjen"), vec![Item::Do(Mark("\n", Line))]);
        assert_eq!(words(&parse("punktet")).join(" "), "punktet");
        assert_eq!(parse("Ny Linje"), vec![Item::Do(Mark("\n", Line))]);
        assert_eq!(words(&parse("komme")).join(" "), "komme");
        assert_eq!(words(&parse("Nøhlinge")).join(" "), "Nøhlinge");
    }

    #[test]
    fn knows_every_english_phrasing() {
        let en = Language::En.profile();
        for (said, action) in super::phrasings(en.commands) {
            let items = super::parse(&format!("a {said} b"), en);
            match action {
                Delete => assert!(!items.contains(&Item::Do(Delete)), "{said}"),
                _ => assert_eq!(items.get(1), Some(&Item::Do(action)), "{said}"),
            }
        }
        // whisper's text is kept as it is, as only edda writes junk
        assert_eq!(
            words(&super::parse("It costs $20, «roughly».", en)).join(" "),
            "It costs $20, «roughly»."
        );
        assert_eq!(super::parse("Scratch that.", en), vec![Item::Do(Delete)]);
    }
}
