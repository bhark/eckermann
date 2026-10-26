//! Local control socket: compositor binds run `eckermann --ptt start|stop|toggle`,
//! and a second plain launch asks the running instance to show its settings.
//! Windows has no socket; the single-instance plugin hands over the command line instead.

#[cfg(unix)]
use std::{
    fs::Permissions,
    io::{BufRead, BufReader, Write},
    os::unix::fs::PermissionsExt,
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    PttStart,
    PttStop,
    PttToggle,
    Show,
}

const NAMES: [(Command, &str); 4] = [
    (Command::PttStart, "start"),
    (Command::PttStop, "stop"),
    (Command::PttToggle, "toggle"),
    (Command::Show, "show"),
];

impl Command {
    #[cfg(any(unix, test))]
    fn name(self) -> &'static str {
        NAMES
            .iter()
            .find(|(c, _)| *c == self)
            .map(|(_, n)| *n)
            .unwrap()
    }

    fn parse(name: &str) -> Option<Self> {
        NAMES.iter().find(|(_, n)| *n == name).map(|(c, _)| *c)
    }
}

/// Command given on our command line; `None` means launch the app.
pub fn from_args(args: &[String]) -> Result<Option<Command>, String> {
    match args {
        [] => Ok(None),
        [flag, action] if flag == "--ptt" => Command::parse(action)
            .filter(|c| *c != Command::Show)
            .map(Some)
            .ok_or_else(|| {
                format!("unknown --ptt action {action:?}, expected start, stop or toggle")
            }),
        _ => Err("usage: eckermann [--ptt start|stop|toggle]".into()),
    }
}

#[cfg(unix)]
fn socket_path() -> PathBuf {
    // per-user dirs: XDG_RUNTIME_DIR on Linux (0700), TMPDIR on macOS
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("eckermann.sock")
}

/// Hand a command to the running instance; fails if none is running.
#[cfg(unix)]
pub fn send(cmd: Command) -> std::io::Result<()> {
    let mut stream = UnixStream::connect(socket_path())?;
    writeln!(stream, "{}", cmd.name())
}

/// Serve commands until the process exits. Call only after `send` failed.
#[cfg(unix)]
pub fn listen(handler: impl Fn(Command) + Send + 'static) -> std::io::Result<()> {
    let path = socket_path();
    // ponytail: two launches in the same instant can both get here; the later one wins the socket
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, Permissions::from_mode(0o600))?;
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                match Command::parse(line.trim()) {
                    Some(cmd) => handler(cmd),
                    None => log::warn!("ignoring unknown control command {line:?}"),
                }
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_cli() {
        assert_eq!(from_args(&args(&[])), Ok(None));
        assert_eq!(
            from_args(&args(&["--ptt", "toggle"])),
            Ok(Some(Command::PttToggle))
        );
        assert!(from_args(&args(&["--ptt", "show"])).is_err());
        assert!(from_args(&args(&["--ptt"])).is_err());
        assert!(from_args(&args(&["start"])).is_err());
    }

    #[test]
    fn names_round_trip() {
        for (cmd, name) in NAMES {
            assert_eq!(Command::parse(cmd.name()), Some(cmd));
            assert_eq!(cmd.name(), name);
        }
    }
}
