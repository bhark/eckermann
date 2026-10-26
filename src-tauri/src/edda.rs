//! Repairs for Edda's text. Edda learned from NST, whose readers said each mark aloud and
//! whose transcripts tag it, so it writes tags like "land,\Komma der" and junk signs.

use crate::commands::Action::Mark;
use crate::commands::Fit::Left;
use crate::commands::{Command, MARKS, phrasings};

// characters kept from whisper; edda writes junk like \ ” § where NST had a tag
const KEEP: [char; 15] = [
    '.', ',', '!', '?', ':', ';', '\'', '-', '–', '/', '(', ')', '%', '&', '+',
];

/// Edda's NST tags made plain words: "land,\Komma der" reads "land komma der".
pub fn plain(phrase: &str, commands: &'static [Command]) -> String {
    let text: String = phrase
        .chars()
        .filter(|&c| c.is_alphanumeric() || c.is_whitespace() || KEEP.contains(&c))
        .collect();
    let names: Vec<&str> = phrasings(commands)
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
                .and_then(|sign| name_of(sign, commands))
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
    words.join(" ")
}

/// The spoken name of a sign whisper writes after words.
fn name_of(sign: char, commands: &'static [Command]) -> Option<&'static str> {
    phrasings(commands)
        .find(|(_, action)| matches!(action, Mark(m, Left) if m.starts_with(sign)))
        .map(|(said, _)| said)
}
