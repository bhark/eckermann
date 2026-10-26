use super::Profile;
use crate::commands::Action::{Delete, Mark};
use crate::commands::Command;
use crate::commands::Fit::{Apart, Join, Left, Line, Quote, Right};
use crate::models;

pub const PROFILE: Profile = Profile {
    code: "da",
    speech: &models::EDDA,
    punct_dir: "punct-da",
    punct_files: &models::PUNCT_DA,
    commands: &COMMANDS,
    compound_joins: &["og", "eller", "samt"],
    nst_tags: true,
    whisper_marks: false,
    // own recordings: 2.9% WER without, 68.9% with
    timestamps: false,
};

const COMMANDS: [Command; 16] = [
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
