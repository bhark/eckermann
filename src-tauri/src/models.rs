//! The model files, fetched once from a pinned revision and checked by size and SHA-256.

use crate::lang::Profile;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};

const BASE: &str =
    "https://huggingface.co/FORNstudio/eckermann/resolve/da4b1baaeddc6f77032f5ce26ab2be72a5b01714";

pub struct File {
    pub path: &'static str,
    size: u64,
    sha256: &'static str,
}

pub const VAD: File = File {
    path: "ggml-silero-v6.2.0.bin",
    size: 885_098,
    sha256: "2aa269b785eeb53a82983a20501ddf7c1d9c48e33ab63a41391ac6c9f7fb6987",
};

pub const EDDA: File = File {
    path: "ggml-edda-v0.2-q8_0.bin",
    size: 874_188_075,
    sha256: "976dd84ac3e4e209d8c77de921d6fd0e42b624470578cd897bef653a97e6b4b7",
};

pub const PUNCT_DA: [File; 3] = [
    File {
        path: "punct-da/config.json",
        size: 1_534,
        sha256: "58df18ea2d40d3f4a23f6549ad68effd35957e9bda5b5690d969b7f4893fac8c",
    },
    File {
        path: "punct-da/model.safetensors",
        size: 220_099_774,
        sha256: "9a7af679dcf1e8b53c6f1a917628f1c5237f89ece496bec12799acca7e128dd1",
    },
    File {
        path: "punct-da/vocab.txt",
        size: 253_349,
        sha256: "b46537f8941a1577817df309c1728ba46adc40947a1112df9c9c3664b45e81e9",
    },
];

pub const TURBO: File = File {
    path: "ggml-large-v3-turbo-q8_0.bin",
    size: 874_188_075,
    sha256: "317eb69c11673c9de1e1f0d459b253999804ec71ac4c23c17ecf5fbe24e259a1",
};

pub const PUNCT_EN: [File; 3] = [
    File {
        path: "punct-en/config.json",
        size: 1_086,
        sha256: "9be3c2cabfbfe94f855ec931b53183b320ebb5418aaec96c81c2587cc5f0623f",
    },
    File {
        path: "punct-en/model.safetensors",
        size: 217_829_558,
        sha256: "65aa41f7ce487063656dbda4b3e78bc54680bae2d91c254d78e6c02545a1256a",
    },
    File {
        path: "punct-en/vocab.txt",
        size: 231_508,
        sha256: "07eced375cec144d27c900241f3e339478dec958f92fddbc551f295c992038a3",
    },
];

/// Files not yet in `dir` that dictating in `profile`'s language needs.
pub fn missing(dir: &Path, profile: &Profile) -> Vec<&'static File> {
    [&VAD, profile.speech]
        .into_iter()
        .chain(profile.punct_files)
        .filter(|f| !std::fs::metadata(dir.join(f.path)).is_ok_and(|m| m.len() == f.size))
        .collect()
}

/// Downloads `files` into `dir`; `progress` gets the bytes done and the bytes in all.
pub async fn fetch(
    dir: &Path,
    files: &[&File],
    mut progress: impl FnMut(u64, u64),
) -> Result<(), String> {
    // ponytail: no resume; an interrupted file starts over, add a Range request if that hurts
    let total = files.iter().map(|f| f.size).sum();
    let client = reqwest::Client::new();
    let mut done = 0;
    for file in files {
        let dest = dir.join(file.path);
        let part = PathBuf::from(format!("{}.part", dest.display()));
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut response = client
            .get(format!("{BASE}/{}", file.path))
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| format!("{}: {e}", file.path))?;
        let mut out = std::fs::File::create(&part).map_err(|e| e.to_string())?;
        let mut hash = Sha256::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| format!("{}: {e}", file.path))?
        {
            out.write_all(&chunk).map_err(|e| e.to_string())?;
            hash.update(&chunk);
            done += chunk.len() as u64;
            progress(done, total);
        }
        out.sync_all().map_err(|e| e.to_string())?;
        if format!("{:x}", hash.finalize()) != file.sha256 {
            let _ = std::fs::remove_file(&part);
            return Err(format!("{}: wrong checksum", file.path));
        }
        std::fs::rename(&part, &dest).map_err(|e| e.to_string())?;
    }
    Ok(())
}
