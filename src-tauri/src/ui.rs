//! The interface's words in the system's language: Danish on a Danish system, else English.
//! The settings page has its own table in src/settings/i18n.ts.

use crate::lang::Language;
use std::sync::OnceLock;

#[derive(Clone, Copy)]
pub enum Text {
    MicFailed,
    ModelFailed,
    PasteByHand,
    PasteFailed,
    Downloading,
    ModelsMissing,
    Settings,
    Quit,
    #[cfg(target_os = "linux")]
    Dictate,
}

/// The system's language, read once.
pub fn language() -> Language {
    static LANGUAGE: OnceLock<Language> = OnceLock::new();
    *LANGUAGE.get_or_init(|| Language::from_locale(&sys_locale::get_locale().unwrap_or_default()))
}

pub fn text(text: Text) -> &'static str {
    // ponytail: a pair per text; a table per language once a third interface language comes
    let (da, en) = match text {
        Text::MicFailed => (
            "Mikrofonen kunne ikke startes.",
            "The microphone could not start.",
        ),
        Text::ModelFailed => (
            "Talemodellen kunne ikke indlæses.",
            "The speech model could not load.",
        ),
        #[cfg(target_os = "macos")]
        Text::PasteByHand => (
            "Teksten ligger i udklipsholderen. Indsæt den med ⌘V.",
            "The text is on the clipboard. Paste it with ⌘V.",
        ),
        #[cfg(not(target_os = "macos"))]
        Text::PasteByHand => (
            "Teksten ligger i udklipsholderen. Indsæt den med Ctrl+V.",
            "The text is on the clipboard. Paste it with Ctrl+V.",
        ),
        Text::PasteFailed => (
            "Teksten kunne ikke indsættes.",
            "The text could not be pasted.",
        ),
        Text::Downloading => (
            "Talemodellerne hentes stadig.",
            "The speech models are still downloading.",
        ),
        Text::ModelsMissing => (
            "Talemodellerne mangler. Hent dem igen i Indstillinger.",
            "The speech models are missing. Download them again in Settings.",
        ),
        Text::Settings => ("Indstillinger", "Settings"),
        Text::Quit => ("Afslut", "Quit"),
        #[cfg(target_os = "linux")]
        Text::Dictate => ("Diktér", "Dictate"),
    };
    match language() {
        Language::Da => da,
        Language::En => en,
    }
}
