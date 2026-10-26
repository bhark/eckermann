use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Append a space after each dictation so consecutive ones don't run together.
    pub trailing_space: bool,
    /// Free the speech model after this many idle minutes.
    pub unload_after_min: u32,
    /// Silence that ends a phrase, which is then written while dictation goes on.
    pub pause_ms: u32,
    /// Dictation key in niri syntax, e.g. "Ctrl+Alt+Space".
    pub hotkey: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            trailing_space: true,
            unload_after_min: 10,
            pause_ms: 800,
            hotkey: None,
        }
    }
}

pub fn load(path: &Path) -> Settings {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            log::warn!("invalid {}: {e}; using defaults", path.display());
            Settings::default()
        }),
        Err(_) => Settings::default(),
    }
}

pub fn save(path: &Path, settings: &Settings) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(settings)?)?;
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_partial_file() {
        let dir = std::env::temp_dir().join(format!("eckermann-settings-{}", std::process::id()));
        let path = dir.join("settings.json");
        let s = Settings {
            trailing_space: false,
            unload_after_min: 3,
            pause_ms: 500,
            hotkey: Some("Ctrl+Alt+Space".into()),
        };
        save(&path, &s).unwrap();
        assert_eq!(load(&path), s);
        std::fs::write(&path, r#"{"unload_after_min": 5}"#).unwrap();
        assert_eq!(
            load(&path),
            Settings {
                unload_after_min: 5,
                ..Settings::default()
            }
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
