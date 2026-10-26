//! What the dictation key runs: `eckermann --ptt ...` without loading webkit and gtk.

use eckermann_lib::ipc;
use std::process::exit;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = match ipc::from_args(&args) {
        Ok(Some(cmd)) => cmd,
        Ok(None) => {
            eprintln!("usage: eckermann-ptt --ptt start|stop|toggle");
            exit(2)
        }
        Err(e) => {
            eprintln!("{e}");
            exit(2)
        }
    };
    if let Err(e) = ipc::send(cmd) {
        eprintln!("eckermann is not running: {e}");
        exit(1)
    }
}
