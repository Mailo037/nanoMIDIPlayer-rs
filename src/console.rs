//! In-app console (the black log box on the player tabs).
//!
//! Like the original, high-frequency logging (every simulated key press) is throttled:
//! only the most recent pending message is kept and flushed by the UI tick, so the
//! playback thread never waits on the UI.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use parking_lot::Mutex;

const WRAP_WIDTH: usize = 27;
const MAX_STORED: usize = 32;

struct State {
    lines: VecDeque<String>,
    pending: Option<String>,
}

static STATE: Mutex<State> = parking_lot::const_mutex(State {
    lines: VecDeque::new(),
    pending: None,
});
static VERSION: AtomicU64 = AtomicU64::new(0);
static ENABLED: AtomicBool = AtomicBool::new(true);

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Throttled log (dropped when newer messages arrive before the next UI tick).
pub fn log(msg: impl Into<String>) {
    let msg = msg.into();
    log::debug!("{msg}");
    if !enabled() {
        return;
    }
    STATE.lock().pending = Some(msg);
}

/// Immediately appends a message, even if the console is disabled in settings
/// (the original `insertConsoleText(..., ignoreConsoleCheck=True)`).
pub fn insert(msg: impl Into<String>) {
    let msg = msg.into();
    log::info!("{msg}");
    push(&mut STATE.lock(), msg);
}

fn push(state: &mut State, msg: String) {
    state.lines.push_back(wrap(&msg));
    while state.lines.len() > MAX_STORED {
        state.lines.pop_front();
    }
    VERSION.fetch_add(1, Ordering::Relaxed);
}

pub fn clear() {
    let mut state = STATE.lock();
    state.lines.clear();
    state.pending = None;
    VERSION.fetch_add(1, Ordering::Relaxed);
}

/// Moves the pending throttled message into the visible list. Called by the UI tick.
pub fn flush() {
    let mut state = STATE.lock();
    if let Some(msg) = state.pending.take() {
        push(&mut state, msg);
    }
}

pub fn version() -> u64 {
    VERSION.load(Ordering::Relaxed)
}

/// Returns the newest `max` messages, oldest first.
pub fn lines(max: usize) -> Vec<String> {
    let state = STATE.lock();
    let skip = state.lines.len().saturating_sub(max);
    state.lines.iter().skip(skip).cloned().collect()
}

/// Short message shown at the bottom of the window for a few seconds.
static TOAST: Mutex<Option<(String, std::time::Instant)>> = parking_lot::const_mutex(None);
static TOAST_VERSION: AtomicU64 = AtomicU64::new(0);
const TOAST_SECS: f32 = 2.8;

pub fn toast(msg: impl Into<String>) {
    *TOAST.lock() = Some((msg.into(), std::time::Instant::now()));
    TOAST_VERSION.fetch_add(1, Ordering::Relaxed);
}

/// The current toast, if it hasn't expired yet.
pub fn current_toast() -> Option<String> {
    let mut t = TOAST.lock();
    if t.as_ref().is_some_and(|(_, at)| at.elapsed().as_secs_f32() > TOAST_SECS) {
        *t = None;
        TOAST_VERSION.fetch_add(1, Ordering::Relaxed);
    }
    t.as_ref().map(|(m, _)| m.clone())
}

pub fn toast_version() -> u64 {
    TOAST_VERSION.load(Ordering::Relaxed)
}

/// All stored messages as plain text (for "Copy log").
pub fn all_text() -> String {
    STATE.lock().lines.iter().cloned().collect::<Vec<_>>().join("\n")
}

/// Hard-wraps text at 27 columns like `textwrap.wrap(..., break_long_words=True)`.
fn wrap(text: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for line in text.split('\n') {
        let mut current = String::new();
        for word in line.split_whitespace() {
            let mut word = word.to_string();
            loop {
                let cur_len = current.chars().count();
                let word_len = word.chars().count();
                let needed = if cur_len == 0 { word_len } else { cur_len + 1 + word_len };
                if needed <= WRAP_WIDTH {
                    if cur_len > 0 {
                        current.push(' ');
                    }
                    current.push_str(&word);
                    break;
                }
                if cur_len > 0 {
                    out.push(std::mem::take(&mut current));
                    continue;
                }
                let head: String = word.chars().take(WRAP_WIDTH).collect();
                word = word.chars().skip(WRAP_WIDTH).collect();
                out.push(head);
                if word.is_empty() {
                    break;
                }
            }
        }
        if !current.is_empty() {
            out.push(current);
        }
    }
    out.join("\n")
}
