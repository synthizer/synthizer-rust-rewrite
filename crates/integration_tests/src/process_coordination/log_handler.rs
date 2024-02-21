//! A log handler which will create a set of files `log/level.txt` where each file contains all logs of that level or
//! lower.
//!
//! Also enables the max log level so that these files always contain data.
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use log::Level;

// We log relative to the beginning of tests, which is set when the log handler is installed by being sure to touch this
// static.
lazy_static::lazy_static! {
    static ref EPOCH: Instant = Instant::now();
}

struct LogHandlerState {
    /// Sorted from Error to Trace.
    files: Vec<(Level, File)>,
}

struct LogHandler {
    state: Mutex<LogHandlerState>,
}

/// Redirects all logs to `ROOT/logs/level.txt`.
pub fn install_log_handler(root: &Path) {
    let logroot = root.join("logs");
    std::fs::create_dir_all(&logroot).unwrap();

    let files = [
        (Level::Error, "error.txt"),
        (Level::Warn, "warn.txt"),
        (Level::Info, "info.txt"),
        (Level::Debug, "debug.txt"),
        (Level::Trace, "trace.txt"),
    ]
    .into_iter()
    .map(|(l, subpath)| {
        let fullpath = logroot.join(subpath);
        let file = File::create(fullpath).expect("Must be able to open logs files to proceed");
        (l, file)
    })
    .collect::<Vec<_>>();

    let handler = LogHandler {
        state: Mutex::new(LogHandlerState { files }),
    };

    log::set_max_level(log::LevelFilter::Trace);
    log::set_boxed_logger(Box::new(handler)).expect("Unable to install logger");
    // Logging this makes sure that the epoch is set.
    log::trace!("Logger installed");
}

impl log::Log for LogHandler {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        // This is a test framework. We always want everything.
        true
    }

    fn flush(&self) {
        // We don't flush because we don't queue.
    }

    fn log(&self, record: &log::Record) {
        let since_epoch = Instant::now() - *EPOCH;
        let since_epoch = chrono::TimeDelta::from_std(since_epoch).unwrap();

        // We will be logging to all of the files, so build this once.
        let message = format!(
            "{since_epoch}: {}: {} (at target {} line {})",
            record.level(),
            record.args(),
            record.target(),
            record.line().unwrap_or(0)
        );

        let mut state = self.state.lock().unwrap();

        for (level, file) in state.files.iter_mut() {
            if record.level() > *level {
                continue;
            }

            writeln!(file, "{message}").expect("Should be able to write log");
        }
    }
}
