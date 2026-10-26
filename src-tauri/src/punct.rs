//! Natural punctuation and capitals: a BERT that gives each word the mark after it and its
//! case, one per language (converted by bench/punct_model.py).

use candle_core::{D, DType, Device, Tensor};
use candle_nn::{Linear, Module, VarBuilder};
use candle_transformers::models::bert::{BertModel, Config};
use std::collections::HashMap;
use std::path::Path;

// 512 positions, less [CLS] and [SEP]
const MAX_PIECES: usize = 510;

/// The mark after a word, and whether the word starts with a capital.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Label {
    pub mark: Option<char>,
    pub capital: bool,
    /// the model starts a sentence after the word (. ? ! or :) and capitalizes the next one
    pub ends_sentence: bool,
}

pub struct Punctuator {
    bert: BertModel,
    classifier: Linear,
    labels: Vec<Label>,
    vocab: HashMap<String, u32>,
    unk: u32,
    cls: u32,
    sep: u32,
}

#[derive(serde::Deserialize)]
struct Head {
    hidden_size: usize,
    id2label: HashMap<usize, String>,
}

impl Punctuator {
    /// `dir` holds model.safetensors, config.json and vocab.txt.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let read = |name: &str| {
            std::fs::read(dir.join(name)).map_err(|e| format!("{}: {e}", dir.join(name).display()))
        };
        let config = read("config.json")?;
        let bert_config: Config = serde_json::from_slice(&config).map_err(|e| e.to_string())?;
        let head: Head = serde_json::from_slice(&config).map_err(|e| e.to_string())?;
        // labels are "<mark or O><U or O>"; colons, dashes and the like come only from spoken commands
        let labels = (0..head.id2label.len())
            .map(|i| {
                let label = head.id2label.get(&i).ok_or(format!("no label {i}"))?;
                let mut chars = label.chars();
                let mark = chars.next();
                Ok(Label {
                    mark: mark.filter(|c| ['.', ',', '?', '!'].contains(c)),
                    capital: chars.next() == Some('U'),
                    ends_sentence: mark.is_some_and(|c| ['.', '?', '!', ':'].contains(&c)),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let vocab: HashMap<String, u32> = String::from_utf8_lossy(&read("vocab.txt")?)
            .lines()
            .zip(0..)
            .map(|(piece, id)| (piece.to_string(), id))
            .collect();
        let id = |piece: &str| {
            vocab
                .get(piece)
                .copied()
                .ok_or(format!("no {piece} in vocab"))
        };
        let (unk, cls, sep) = (id("[UNK]")?, id("[CLS]")?, id("[SEP]")?);
        let err = |e: candle_core::Error| e.to_string();
        let vb = VarBuilder::from_buffered_safetensors(
            read("model.safetensors")?,
            DType::F32,
            &Device::Cpu,
        )
        .map_err(err)?;
        Ok(Self {
            bert: BertModel::load(vb.clone(), &bert_config).map_err(err)?,
            classifier: candle_nn::linear(head.hidden_size, labels.len(), vb.pp("classifier"))
                .map_err(err)?,
            labels,
            vocab,
            unk,
            cls,
            sep,
        })
    }

    /// One label per word; earlier words are context for later ones.
    pub fn label(&self, words: &[String]) -> Result<Vec<Label>, String> {
        let pieces: Vec<Vec<u32>> = words.iter().map(|w| self.pieces(w)).collect();
        let mut out = Vec::with_capacity(words.len());
        let mut from = 0;
        while from < pieces.len() {
            // ponytail: windows do not overlap, so a word at a window's end lacks right context; only phrases of hundreds of words hit it
            let mut to = from;
            let mut len = 0;
            while to < pieces.len() && (to == from || len + pieces[to].len() <= MAX_PIECES) {
                len += pieces[to].len();
                to += 1;
            }
            out.extend(self.run(&pieces[from..to]).map_err(|e| e.to_string())?);
            from = to;
        }
        Ok(out)
    }

    fn run(&self, words: &[Vec<u32>]) -> candle_core::Result<Vec<Label>> {
        let mut ids = vec![self.cls];
        let mut firsts = Vec::with_capacity(words.len());
        for pieces in words {
            firsts.push(ids.len());
            ids.extend(pieces);
        }
        ids.truncate(MAX_PIECES + 1);
        ids.push(self.sep);
        let ids = Tensor::new(ids.as_slice(), &Device::Cpu)?.unsqueeze(0)?;
        let hidden = self.bert.forward(&ids, &ids.zeros_like()?, None)?;
        let best: Vec<u32> = self
            .classifier
            .forward(&hidden)?
            .squeeze(0)?
            .argmax(D::Minus1)?
            .to_vec1()?;
        // a word's label sits on its first piece
        Ok(firsts
            .iter()
            .map(|&i| {
                best.get(i)
                    .map_or_else(Label::default, |&l| self.labels[l as usize])
            })
            .collect())
    }

    /// WordPiece ids of the word's letters and digits, as BERT's tokenizer splits them.
    fn pieces(&self, word: &str) -> Vec<u32> {
        let word = word.to_lowercase();
        let mut out = Vec::new();
        for part in word
            .split(|c: char| !c.is_alphanumeric())
            .filter(|p| !p.is_empty())
        {
            let start = out.len();
            let mut rest = part;
            while !rest.is_empty() {
                let found = rest
                    .char_indices()
                    .map(|(i, c)| i + c.len_utf8())
                    .rev()
                    .find_map(|end| {
                        let piece = match rest.len() == part.len() {
                            true => rest[..end].to_string(),
                            false => format!("##{}", &rest[..end]),
                        };
                        Some((*self.vocab.get(&piece)?, end))
                    });
                let Some((id, end)) = found else {
                    out.truncate(start);
                    out.push(self.unk);
                    break;
                };
                out.push(id);
                rest = &rest[end..];
            }
        }
        if out.is_empty() {
            out.push(self.unk);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(folder: &str) -> Option<Punctuator> {
        let home = std::env::var_os("HOME").unwrap_or_default();
        let dir = Path::new(&home)
            .join(".local/share/dk.forn.eckermann/models")
            .join(folder);
        Punctuator::load(&dir)
            .inspect_err(|_| eprintln!("skipped: no model in {}", dir.display()))
            .ok()
    }

    #[test]
    fn splits_english_words_like_bert() {
        let Some(p) = load("punct-en") else {
            return;
        };
        // ids from python's BertTokenizer, fed words as the app feeds them
        let cases: [(&str, &[u32]); 3] = [
            (
                "rudolf steiner said that goethe's work",
                &[12794, 21264, 2056, 2008, 23173, 1055, 2147],
            ),
            (
                "it's a black-white film in aarhus",
                &[2009, 1055, 1037, 2304, 2317, 2143, 1999, 29173],
            ),
            (
                "xqzzyvw 2026 kierkegaard",
                &[
                    1060, 4160, 28753, 2615, 2860, 16798, 2575, 11382, 2121, 3489, 18839,
                ],
            ),
        ];
        for (text, want) in cases {
            let got: Vec<u32> = text.split(' ').flat_map(|w| p.pieces(w)).collect();
            assert_eq!(got, want, "{text}");
        }
        let words: Vec<String> = "what is your name my name is peter"
            .split(' ')
            .map(String::from)
            .collect();
        let labels = p.label(&words).unwrap();
        assert_eq!(labels[3].mark, Some('?'));
        assert!(labels[7].capital);
    }

    #[test]
    fn splits_words_like_bert() {
        let Some(p) = load("punct-da") else {
            return;
        };
        // ids from python's BertTokenizer on the same vocab
        let cases: [(&str, &[u32]); 3] = [
            (
                "rudolf steiner han mente at goethes værk",
                &[12275, 3841, 23581, 5, 165, 5056, 39, 248, 21392, 29, 3661],
            ),
            (
                "det er jens' sort-hvide bil på øen",
                &[46, 33, 4059, 2301, 3044, 340, 58, 6444],
            ),
            (
                "xqzzyvw 2026 kierkegaard",
                &[1283, 31747, 31220, 31707, 31728, 19987, 31736, 24917],
            ),
        ];
        for (text, want) in cases {
            let got: Vec<u32> = text.split(' ').flat_map(|w| p.pieces(w)).collect();
            assert_eq!(got, want, "{text}");
        }
        let words: Vec<String> = "hvad hedder du jeg hedder peter"
            .split(' ')
            .map(String::from)
            .collect();
        let labels = p.label(&words).unwrap();
        assert_eq!(labels[2].mark, Some('?'));
        assert!(labels[5].capital);
    }
}
