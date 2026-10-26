//! Global dictation key. Wayland apps cannot grab keys, so on niri Eckermann writes
//! its own bind file for config.kdl to include; elsewhere the user binds the command.

use serde::Serialize;
use std::path::PathBuf;
use std::process::Command;

const NIRI_FILE: &str = "eckermann.kdl";

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Status {
    Niri {
        included: bool,
        include_line: String,
    },
    Manual {
        command: String,
    },
}

pub fn status() -> Status {
    match niri_config() {
        Some(config) => Status::Niri {
            included: std::fs::read_to_string(&config)
                .is_ok_and(|text| text.contains(&format!("\"{NIRI_FILE}\""))),
            include_line: format!("include optional=true \"{NIRI_FILE}\""),
        },
        None => Status::Manual {
            command: format!("{} --ptt toggle", sender().display()),
        },
    }
}

/// Binds `combo` (niri key syntax, e.g. "Ctrl+Alt+Space") to start and stop dictation.
pub fn apply(combo: &str) -> Result<(), String> {
    // goes into the KDL file unquoted
    if combo.is_empty()
        || !combo
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '_')
    {
        return Err(format!("invalid key combination {combo:?}"));
    }
    let Some(config) = niri_config() else {
        return Ok(());
    };
    let file = config.with_file_name(NIRI_FILE);
    let tmp = file.with_extension("kdl.tmp");
    // ponytail: toggle only; niri 26.04 cannot bind key release, so no hold-to-talk yet
    // repeat=false: a held key would otherwise toggle at the repeat rate
    let body = format!(
        "// written by Eckermann; change the key in its settings\nbinds {{\n    {combo} repeat=false {{ spawn {:?} \"--ptt\" \"toggle\"; }}\n}}\n",
        sender().to_string_lossy()
    );
    // unchanged: skip niri validate and the config reload a rename triggers
    if std::fs::read_to_string(&file).is_ok_and(|old| old == body) {
        return Ok(());
    }
    std::fs::write(&tmp, body).map_err(|e| e.to_string())?;
    // validate standalone before the rename, so niri never live-reloads a broken bind
    let out = Command::new("niri")
        .arg("validate")
        .arg("-c")
        .arg(&tmp)
        .output()
        .map_err(|e| format!("niri validate: {e}"))?;
    if !out.status.success() {
        let _ = std::fs::remove_file(&tmp);
        log::warn!(
            "niri rejected {combo}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        return Err(format!("niri does not accept the key combination {combo}"));
    }
    std::fs::rename(&tmp, &file).map_err(|e| e.to_string())
}

fn niri_config() -> Option<PathBuf> {
    std::env::var_os("NIRI_SOCKET")?;
    if let Some(path) = std::env::var_os("NIRI_CONFIG") {
        return Some(path.into());
    }
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::home_dir().map(|home| home.join(".config")))?;
    Some(config_home.join("niri/config.kdl"))
}

/// The small sender beside us, else the app itself.
fn sender() -> PathBuf {
    // inside an AppImage current_exe is a temporary mount path
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        return appimage.into();
    }
    let exe = std::env::current_exe().unwrap_or_else(|_| "eckermann".into());
    // cargo run builds only the app
    let sender = exe.with_file_name("eckermann-ptt");
    if sender.exists() { sender } else { exe }
}
