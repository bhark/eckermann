//! Spoken commands: words that type a mark or edit the text instead of themselves.

use Action::{Delete, Mark};
use Fit::{Apart, Join, Left, Line, Quote, Right};

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

/// Every command with its phrasings, in lowercase.
const COMMANDS: [(&[&str], Action); 16] = [
    (&["nyt afsnit"], Mark("\n\n", Line)),
    (
        &[
            "ny linje",
            "ny linie",
            "nye linje",
            "nye linie",
            "linjeskift",
            "linieskift",
        ],
        Mark("\n", Line),
    ),
    (&["punktum"], Mark(".", Left)),
    (&["komma", "comma"], Mark(",", Left)),
    (&["spørgsmålstegn"], Mark("?", Left)),
    (&["udråbstegn"], Mark("!", Left)),
    (&["kolon"], Mark(":", Left)),
    (&["semikolon"], Mark(";", Left)),
    (&["apostrof"], Mark("'", Left)),
    (&["bindestreg"], Mark("-", Join)),
    (&["skråstreg"], Mark("/", Join)),
    (&["tankestreg"], Mark("–", Apart)),
    (&["citationstegn", "anførselstegn"], Mark("\"", Quote)),
    (&["start parentes", "parentes start"], Mark("(", Right)),
    (&["slut parentes", "parentes slut"], Mark(")", Left)),
    (&["slet"], Delete),
];

// characters kept from whisper; edda writes junk like \ ” § where NST had a tag
const KEEP: [char; 15] = [
    '.', ',', '!', '?', ':', ';', '\'', '-', '–', '/', '(', ')', '%', '&', '+',
];

#[derive(Debug, PartialEq)]
pub enum Item {
    Word(String),
    Do(Action),
}

/// Splits a transcribed phrase into words and commands. Editing commands count only
/// in a phrase of commands alone, so "hun kom slet ikke" stays text.
pub fn parse(phrase: &str) -> Vec<Item> {
    let text = plain(phrase);
    let words: Vec<&str> = text.split_whitespace().collect();
    let all = scan(&words, true);
    if all.iter().all(|item| matches!(item, Item::Do(_))) {
        return all;
    }
    let heard_one = all.iter().any(|item| matches!(item, Item::Do(_)));
    if let Some(action) = misheard(&words).filter(|_| !heard_one) {
        return vec![Item::Do(action)];
    }
    scan(&words, false)
}

fn scan(words: &[&str], editing: bool) -> Vec<Item> {
    let mut items = Vec::new();
    let mut rest = words;
    while let Some(first) = rest.first() {
        match command_at(rest).filter(|&(_, action)| editing || action != Delete) {
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

fn phrasings() -> impl Iterator<Item = (&'static str, Action)> {
    COMMANDS
        .iter()
        .flat_map(|(said, action)| said.iter().map(move |s| (*s, *action)))
}

/// The command that `words` start with, and how many words it took; the longest wins.
fn command_at(words: &[&str]) -> Option<(usize, Action)> {
    // a lone sign is edda writing the spoken word as its mark
    if let Some(sign) = words
        .first()
        .filter(|w| w.len() == 1 && w.starts_with(MARKS))
    {
        return phrasings()
            .find(|(_, action)| matches!(action, Mark(m, Left) if m == sign))
            .map(|(_, action)| (1, action));
    }
    phrasings()
        .filter_map(|(said, action)| {
            let n = said.split(' ').count();
            let hit = n <= words.len() && said.split(' ').zip(words).all(|(s, w)| core(w) == s);
            hit.then_some((n, action))
        })
        .max_by_key(|&(n, _)| n)
        .or_else(|| {
            // long names are distinctive, so a word close to one is that command misheard
            let heard: Vec<char> = core(words[0]).chars().collect();
            phrasings()
                .filter(|(said, _)| !said.contains(' '))
                .find(|(said, _)| {
                    let said: Vec<char> = said.chars().collect();
                    said.len() >= 9 && distance(&heard, &said) <= said.len() / 5
                })
                .map(|(_, action)| (1, action))
        })
}

/// A short phrase said alone may be a misheard command, such as "nye linjer".
fn misheard(words: &[&str]) -> Option<Action> {
    if words.is_empty() || words.len() > 3 {
        return None;
    }
    let heard: Vec<char> = words
        .iter()
        .flat_map(|w| core(w).chars().collect::<Vec<_>>())
        .collect();
    phrasings()
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

/// Whisper's text with edda's NST tags made plain words: "land,\Komma der" reads
/// "land komma der". Edda learned the tags from NST, whose readers said each mark aloud.
fn plain(phrase: &str) -> String {
    let text: String = phrase
        .chars()
        .filter(|&c| c.is_alphanumeric() || c.is_whitespace() || KEEP.contains(&c))
        .collect();
    let names: Vec<&str> = phrasings()
        .filter(|(said, action)| !said.contains(' ') && matches!(action, Mark(..)))
        .map(|(said, _)| said)
        .collect();
    // a tag is a mark's name glued to a sign, or capitalized right after a word
    let tag_at = |rest: &str| {
        rest.char_indices().find_map(|(i, c)| {
            let before = rest[..i].chars().last()?;
            let glued = MARKS.contains(&before)
                || before == '-'
                || (before.is_lowercase() && c.is_uppercase());
            let name = names.iter().find(|name| {
                let ends = |end: &str| end.chars().next().is_none_or(|c| !c.is_lowercase());
                rest.get(i..i + name.len())
                    .is_some_and(|w| w.to_lowercase() == **name && ends(&rest[i + name.len()..]))
            })?;
            glued.then_some((i, *name))
        })
    };
    let mut out = String::new();
    let mut rest = text.as_str();
    while let Some((at, tag)) = tag_at(rest) {
        let bare = rest[..at].trim_end_matches(MARKS);
        // at a pause edda writes ",.\Punktum", the sign said and its habitual end
        let said = match tag {
            "punktum" => rest[bare.len()..at]
                .chars()
                .find(|&c| c != '.')
                .and_then(name_of)
                .unwrap_or(tag),
            _ => tag,
        };
        out += &format!("{bare} {said} ");
        rest = &rest[at + tag.len()..];
    }
    out += rest;
    // edda glues signs to the next word when it drops a tag, as in "følgende,mælk"
    let chars: Vec<char> = out.chars().collect();
    let mut out = String::new();
    for (i, &c) in chars.iter().enumerate() {
        out.push(c);
        let between = i > 0
            && chars[i - 1].is_alphabetic()
            && chars.get(i + 1).is_some_and(|n| n.is_alphabetic());
        if between && c != '.' && MARKS.contains(&c) {
            out.push(' ');
        }
    }
    // runs left from removed junk, as in "land,,", stand for one sign
    let words: Vec<String> = out
        .split_whitespace()
        .map(|word| {
            let bare = word.trim_end_matches(MARKS);
            let run = &word[bare.len()..];
            match run.len() > 1 {
                true => format!("{bare}{}", run.chars().find(|&c| c != '.').unwrap_or('.')),
                false => word.to_string(),
            }
        })
        .collect();
    let mut text = String::new();
    for (i, word) in words.iter().enumerate() {
        text += word;
        // "sort- hvid" is a spoken bindestreg; a split compound goes on with a conjunction
        let compound = words.get(i + 1).is_some_and(|next| {
            let next = core(next);
            let command =
                phrasings().any(|(said, _)| said.split(' ').next() == Some(next.as_str()));
            !command && !["og", "eller", "samt"].contains(&next.as_str())
        });
        if !(compound && word.len() > 1 && word.ends_with('-')) {
            text.push(' ');
        }
    }
    text
}

/// The spoken name of a sign whisper writes after words.
fn name_of(sign: char) -> Option<&'static str> {
    phrasings()
        .find(|(_, action)| matches!(action, Mark(m, Left) if m.starts_with(sign)))
        .map(|(said, _)| said)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
