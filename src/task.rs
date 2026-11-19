//! What the OS scheduler runs: `nbw-gui --task` (periodic) and `nbw-gui --task --if-due`
//! (after login). The GUI program has no console window on Windows, so nothing flashes on
//! screen, and it handles these arguments before any window or web view is created. Each
//! outcome is logged, with its time, to `last-run.log`.

use crate::engine::{self, Outcome};
use crate::{config, error, platform};
use std::time::Duration;

/// Argument that makes the GUI program do one scheduled run and exit.
pub const TASK_ARG: &str = "--task";
/// Added for the after-login check.
pub const IF_DUE_ARG: &str = "--if-due";

/// How many lines `last-run.log` keeps.
const LOG_LINES: usize = 200;

/// One scheduled run. `if_due` is the after-login check: it retries network failures while
/// the network may still be coming up.
pub fn run(if_due: bool) {
    let outcome = if if_due {
        let mut attempt = 0;
        loop {
            let res = engine::run_if_due();
            attempt += 1;
            // Only a network failure is worth waiting for; anything else will not fix itself.
            match &res {
                Err(e) if attempt < 4 && error::is_network(e) => std::thread::sleep(Duration::from_secs(20)),
                _ => break res,
            }
        }
    } else {
        engine::run_scheduled()
    };

    let what = match outcome {
        Ok(Outcome::Changed(cur)) => format!("ok: {}", cur.title),
        Ok(Outcome::Skipped(why)) => format!("skipped: {why}"),
        Err(e) => format!("error: {e:#}"),
    };
    let kind = if if_due { "login" } else { "periodic" };
    log(&format!("{} [{kind}] {what}", platform::local_now()));
}

/// Append a line to `last-run.log`, keeping only the most recent lines. The periodic and the
/// after-login run can finish at the same moment: a lock keeps one from dropping the other's line.
fn log(line: &str) {
    let Ok(_lock) = config::lock_file("last-run.lock", Duration::from_secs(30)) else { return };
    let Ok(dir) = config::data_dir() else { return };
    let path = dir.join("last-run.log");
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<&str> = old.lines().collect();
    lines.push(line);
    let start = lines.len().saturating_sub(LOG_LINES);
    let _ = config::write_atomic(&path, (lines[start..].join("\n") + "\n").as_bytes());
}
