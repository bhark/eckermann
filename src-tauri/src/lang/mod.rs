//! What dictating in a language needs: its models, its spoken commands and the spelling
//! the joiner keeps. A new language is a file here plus its model files in `models`.

mod da;
mod en;

use crate::commands::Command;
use crate::models::File;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Da,
    En,
}

pub struct Profile {
    /// whisper's code for the language
    pub code: &'static str,
    pub speech: &'static File,
    /// the punctuation model's folder, and the files in it
    pub punct_dir: &'static str,
    pub punct_files: &'static [File],
    pub commands: &'static [Command],
    /// words that go on after a split compound, as in "bevarelses- og adgangsgebyrer"
    pub compound_joins: &'static [&'static str],
    /// the speech model writes edda's NST tags and junk signs, which need repair first
    pub nst_tags: bool,
    /// the speech model's marks and capitals hold inside a phrase; the labeler only joins phrases
    pub whisper_marks: bool,
    /// decode with timestamps, which keep turbo from skipping a sentence but break edda
    pub timestamps: bool,
}

pub const ALL: [Language; 2] = [Language::Da, Language::En];

impl Language {
    pub fn profile(self) -> &'static Profile {
        match self {
            Self::Da => &da::PROFILE,
            Self::En => &en::PROFILE,
        }
    }

    /// The language of a BCP 47 locale such as "da-DK", English when there is none.
    pub fn from_locale(locale: &str) -> Self {
        match locale.split(['-', '_']).next() {
            Some("da") => Self::Da,
            _ => Self::En,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn punct_files_are_in_their_folder() {
        for language in ALL {
            let p = language.profile();
            for f in p.punct_files {
                assert!(
                    f.path.starts_with(&format!("{}/", p.punct_dir)),
                    "{}",
                    f.path
                );
            }
        }
    }

    #[test]
    fn reads_the_locale() {
        assert_eq!(Language::from_locale("da-DK"), Language::Da);
        assert_eq!(Language::from_locale("en-US"), Language::En);
        assert_eq!(Language::from_locale("de-DE"), Language::En);
        assert_eq!(Language::from_locale(""), Language::En);
    }
}
