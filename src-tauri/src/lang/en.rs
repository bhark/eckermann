use super::Profile;
use crate::commands::Action::{Delete, Mark};
use crate::commands::Command;
use crate::commands::Fit::{Join, Left, Line, Quote, Right};
use crate::models;

pub const PROFILE: Profile = Profile {
    code: "en",
    speech: &models::TURBO,
    punct_dir: "punct-en",
    punct_files: &models::PUNCT_EN,
    commands: &COMMANDS,
    compound_joins: &["and", "or"],
    nst_tags: false,
    // turbo's own marks beat the labeler's inside phrases on FLEURS en: 139 against 178 mark errors
    whisper_marks: true,
    // FLEURS en pairs: 15 of 60 lost a sentence without, 3 with
    timestamps: true,
};

// as macOS Dictation says them; "dash" and "quote" alone are everyday words
const COMMANDS: [Command; 16] = [
    (&["new paragraph"], Mark("\n\n", Line)),
    (&["new line", "newline", "line break"], Mark("\n", Line)),
    (&["period", "full stop"], Mark(".", Left)),
    (&["comma"], Mark(",", Left)),
    (&["question mark"], Mark("?", Left)),
    (&["exclamation mark", "exclamation point"], Mark("!", Left)),
    (&["colon"], Mark(":", Left)),
    (&["semicolon"], Mark(";", Left)),
    (&["apostrophe"], Mark("'", Left)),
    (&["hyphen"], Mark("-", Join)),
    (&["slash"], Mark("/", Join)),
    (&["em dash"], Mark("—", Join)),
    (
        &["open quote", "close quote", "end quote", "unquote"],
        Mark("\"", Quote),
    ),
    (&["open parenthesis", "open paren"], Mark("(", Right)),
    (&["close parenthesis", "close paren"], Mark(")", Left)),
    (&["delete that", "scratch that", "delete"], Delete),
];
