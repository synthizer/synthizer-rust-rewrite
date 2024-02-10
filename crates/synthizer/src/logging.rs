//! Internal module to handle logging.
//!
//! Synthizer has a problem.  It wishes to be able to log from the audio thread.  The audio thread cannot allocate or do
//! I/O.  Rust's logging facades and the tracing crate absolutely 100% do not support this in any way.  Logging kind of
//! does, but only if the handler which is installed doesn't do bad things.
//!
//! But, as proven originally in the C++ version of Synthizer and by about 0 seconds of thought, we need to do
//! *something*.  The best we can do here is a ringbuffer.  Specifically, we define some macros `syz_level` which work
//! (mostly) like the macros from the log crate:
//!
//! - On non-audio threads, these are very boring and forward to the macros from the log crate.
//! - On audio threads, these are very interesting and instead forward to a ringbuffer with a fixed-size message limit,
//!   eventually but not immediately read by a background thread which will convert them to the log crate.
//!
//! We make sure to indicate that messages were truncated when converting to the log crate.  We also detect messages we
//! missed because the background thread was too slow.  We then tune things such that we'll only miss messages if it's
//! the case that there's a flood.
//!
//! There are two very unfortunate design problems here:
//!
//! - We can't integrate with tracing.  It'd be really nice if we could, but tracing fundamentally wants an allocator.
//! - We can't correct timestamps.  The timestamps the user sees are the timestamp that the log crate got the message,
//!   not when it happened.  The only other alternative would be to not use the log crate, but that's terrible ux,
//!   certainly worse than slight delays.
//!
//! We deal with the timestamp issue by carefully utilizing thread waking.  When an audio tick ends, we poke a thread
//! token to hit the logging thread.  See the docs in audio_synchronization's mpsc_counter as to why we consider this
//! safe; the short version is that an audit shows that it is, and in future if it's not we can write our own
//! alternatives.  When not under load, the thread wakes and begins spitting logs out as soon as the audio tick ends
//! (when under load the OS can preempt it, we can't control that).  When we detect large delays, we add an informative
//! message to the end of the log messages which were delayed.
use std::fmt::Arguments as FmtArgs;
use std::thread::{park, JoinHandle};
use std::time::{Duration, Instant};

use arrayvec::ArrayString;
use thingbuf::{recycling::Recycle, ThingBuf};

use crate::is_audio_thread::is_audio_thread;

// The following two values reserve around `LOG_LENGTH_LIMIT * LOG_QUEUE_LENGTH` bytes for the log queue.

const LOG_LENGTH_LIMIT: usize = 512;
const LOG_QUEUE_LENGTH: usize = 16384;

/// If logging falls this far behind, start warning the user.
const WARN_LATENCY: Duration = Duration::from_millis(250);

type InlineLogMessage = ArrayString<LOG_LENGTH_LIMIT>;

/// A log message can either be a fixed-size static string, or something formatted to an inline buffer.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)] //because this is basically Cow.
pub(crate) enum LogMessage {
    Static(&'static str),
    Inline(InlineLogMessage),
}

struct LogRecordRecycler;

pub(crate) struct LogRecord {
    /// If a thread detects that it was unable to enqueue messages, it sets this value.
    skipped_messages: u64,

    level: log::Level,

    /// Output of the `module_path!` macro.
    module: &'static str,

    message: LogMessage,

    /// This message might have been truncated. Was it?
    truncated: bool,

    /// The instant at which this log message was put together and enqueued.
    enqueue_time: Instant,
}

/// The arrayvec crate does not support formatting in a way which would let us detect truncations.  This formatter
/// pushes things to a log message until it's full, then sets truncated to true.
///
/// On truncation, it just keeps going and throws out the values.
struct LogMessageFormatter<'a> {
    log_message: &'a mut InlineLogMessage,
    truncated: &'a mut bool,
}

impl<'a> std::fmt::Write for LogMessageFormatter<'a> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        // This formatter never fails. On truncation, it just keeps going and throws the values away.
        if *self.truncated {
            return Ok(());
        }

        let remaining = self.log_message.remaining_capacity();
        // Careful: ArrayString capacity is in bytes.
        if s.as_bytes().len() <= remaining {
            self.log_message.push_str(s);
            return Ok(());
        }

        *self.truncated = true;

        // Otherwise, we are truncating. To do so, we will unfortunately have to push characters until we can't anymore.
        // arrayvec doesn't offer us a good API for this, and we want to preserve character boundaries.  The easiest way
        // is to therefore go char by char.
        for c in s.chars() {
            if self.log_message.try_push(c).is_err() {
                return Ok(());
            }
        }

        Ok(())
    }
}

/// Build a log message.
///
/// The returned message has skipped_messages set to 0. This is then fixed up by the caller, where the actual enqueueing
/// happens.
pub(crate) fn build_log_message(
    level: log::Level,
    args: FmtArgs<'_>,
    module: &'static str,
) -> LogRecord {
    use std::fmt::Write;

    let mut truncated = false;

    let message = match args.as_str() {
        Some(m) => LogMessage::Static(m),
        None => {
            let mut buf = InlineLogMessage::new();

            let mut formatter = LogMessageFormatter {
                truncated: &mut truncated,
                log_message: &mut buf,
            };

            write!(formatter, "{}", args).expect("Our formatter never fails");

            LogMessage::Inline(buf)
        }
    };

    LogRecord {
        skipped_messages: 0,
        level,
        message,
        module,
        truncated,
        enqueue_time: Instant::now(),
    }
}

struct LogMessageRecycler;

impl Recycle<LogRecord> for LogRecordRecycler {
    fn new_element(&self) -> LogRecord {
        LogRecord {
            skipped_messages: 0,
            level: log::Level::Debug,
            enqueue_time: Instant::now(),
            message: LogMessage::Static("NOT SET"),
            module: module_path!(),
            truncated: false,
        }
    }

    fn recycle(&self, _element: &mut LogRecord) {
        // No-op; we'll just overwrite it on the next time round.
    }
}

/// Global state stored in a static for the logger.
struct LogCtx {
    /// Wake this thread when messages arrive.
    thread: JoinHandle<()>,

    /// Send messages here, if possible.
    message_queue: ThingBuf<LogRecord, LogRecordRecycler>,
}

fn setup_ctx() -> LogCtx {
    // This isn't recursive access to the lazy_static because the "recursive" part is in the background thread, which
    // will block until we finish.
    let thread = std::thread::spawn(background_thread_mainloop);
    let message_queue = ThingBuf::with_recycle(LOG_QUEUE_LENGTH, LogRecordRecycler);

    LogCtx {
        thread,
        message_queue,
    }
}

lazy_static::lazy_static! {
    static ref GLOBAL_CTX: LogCtx = setup_ctx();
}

/// Dispatch a log message to the background thread if needed.
///
/// This is the entrypoint for the macro.
pub(crate) fn dispatch_message(level: log::Level, args: FmtArgs<'_>, module: &'static str) {
    use std::cell::Cell;

    thread_local! {
        static SKIPPED_MESSAGES: Cell<u64> = const { Cell::new(0) };
    }

    if level > log::max_level() {
        return;
    }

    // Otherwise let's try to enqueue it.
    let mut record = build_log_message(level, args, module);
    record.skipped_messages = SKIPPED_MESSAGES.get();

    match GLOBAL_CTX.message_queue.push(record) {
        Ok(_) => {
            // Finally told the background thread about skipped messages.
            SKIPPED_MESSAGES.replace(0);
            GLOBAL_CTX.thread.thread().unpark();
        }
        Err(_) => {
            SKIPPED_MESSAGES.replace(SKIPPED_MESSAGES.get() + 1);
        }
    }
}

/// Convert a single log message to the log crate's macros and spit it out.
fn log_one(record: LogRecord) {
    let msg_str = match &record.message {
        LogMessage::Static(s) => s,
        LogMessage::Inline(i) => i.as_str(),
    };

    let latency = Instant::now() - record.enqueue_time;

    if record.skipped_messages != 0 {
        log::warn!(
            "Synthizer's background logging thread fell behind!  {} messages have been dropped!",
            record.skipped_messages
        );
    }

    let mut latency_part_bytes: smallvec::SmallVec<[u8; 256]> = smallvec::SmallVec::new();

    if latency > WARN_LATENCY {
        use std::io::Write;

        write!(
            latency_part_bytes,
            ", delayed by {} seconds",
            latency.as_secs_f64()
        )
        .expect("Writing to a smallvec shouldn't fail");
    }

    let latency_part = std::str::from_utf8(&latency_part_bytes[..])
        .expect("Rust formatting only ever writes valid UTF8");

    let truncated_part = if record.truncated { ", truncated" } else { "" };

    log::log!(target: record.module, record.level, "{} (from rt thread{latency_part}{truncated_part})", msg_str);
}

fn drain_queue() {
    while let Some(msg) = GLOBAL_CTX.message_queue.pop() {
        log_one(msg);
    }
}

/// Drain the queue. Park. Repeat forever.
fn background_thread_mainloop() {
    loop {
        drain_queue();
        // If another message gets between draining and parking, the thread's token has returned before the park and all is well.
        park();
    }
}

/// Same as the log macro, but realtime-safe and the target is always the current module.
#[allow(clippy::crate_in_macro_def)] // This is private.
macro_rules! rt_log {
    ($level: expr, $fmt: expr $(, $args: expr)* $(,)?) => {
        let macro_level = $level;
        if crate::is_audio_thread::is_audio_thread() && macro_level <= log::max_level() {
            crate::logging::dispatch_message(macro_level, format_args!($fmt, $($args),*), module_path!());
        } else {
            log::log!($level, $fmt, $($args),*);
        }
    }
}

macro_rules! rt_error {
 ($($arg: tt)+) => {
        rt_log!(log::Level::Error, $($arg)*);
    }
}

macro_rules! rt_warn {
    ($($args:tt)+) => {
        rt_log!(log::Level::Warn, $($args)*);
    }
}

macro_rules! rt_info {
    ($($args: tt)+) => {
        rt_log!(log::Level::Info, $($args)*);
    }
}

macro_rules! rt_debug {
    ($($args: tt)+) => {
        rt_log!(log::Level::Debug, $($args)*);
    }
}

macro_rules! rt_trace {
    ($($args: tt)+) => {
        rt_log!(log::Level::Trace, $($args)*);
    }
}

/// If this compiles, we can at least know that our macros can build, but we don't otherwise call it.
///
/// In other words, it's a "test".
#[allow(dead_code)]
fn test_macros_build() {
    macro_rules! tester {
        ($mac: tt) => {
            $mac!("hello");
            $mac!("hello {}", 5);
            $mac!("hello {}", 5,);
        };
    }

    tester!(rt_error);
    tester!(rt_warn);
    tester!(rt_info);
    tester!(rt_debug);
    tester!(rt_trace);
}

/// Ensure that the lazy_static is set up so that this module works.
///
/// Called when constructing servers, so that we can be ensured that the log module spawns not on an audio thread.
pub(crate) fn ensure_log_ctx() {
    // black_box is probably unnecessary, but "touch the lazy_static" is unusual so let's be a bit paranoid.
    std::hint::black_box(GLOBAL_CTX.message_queue.capacity());
}
