//! Start menu model: open state, search filtering and keyboard selection.
//!
//! The window manager owns the visuals and hit-testing; this module owns the
//! menu's state so keyboard and mouse input share one selection.

use crate::sync::SpinLock;

pub const APPS: [(&str, u32); 7] = [
    ("Terminal", 0x0092_C3FF),
    ("Monitor", 0x0085_DDD0),
    ("Settings", 0x00BE_AAFF),
    ("About", 0x00F5_AAD5),
    ("Installer", 0x00FF_BA8C),
    ("Browser", 0x008B_D7ED),
    ("File Manager", 0x00ED_CC8B),
];

pub const APP_COUNT: usize = APPS.len();
const SEARCH_CAPACITY: usize = 16;

struct State {
    session: Option<usize>,
    open: bool,
    selection: usize,
    search: [u8; SEARCH_CAPACITY],
    search_len: usize,
}

static STATE: SpinLock<State> = SpinLock::new(State {
    session: None,
    open: false,
    selection: 0,
    search: [0; SEARCH_CAPACITY],
    search_len: 0,
});

/// Whether the Start menu is currently visible.
pub fn is_open() -> bool {
    STATE.lock().open
}

/// Open the Start menu with a clean search field.
pub fn open() {
    let mut state = STATE.lock();
    state.session = None;
    state.open = true;
    state.selection = 0;
    state.search = [0; SEARCH_CAPACITY];
    state.search_len = 0;
}

pub fn close() {
    STATE.lock().open = false;
}

/// Toggle the menu. Returns the new open state.
pub fn toggle() -> bool {
    let mut state = STATE.lock();
    state.session = None;
    state.open = !state.open;
    if state.open {
        state.selection = 0;
        state.search = [0; SEARCH_CAPACITY];
        state.search_len = 0;
    }
    state.open
}

fn lowercase(byte: u8) -> u8 {
    if byte.is_ascii_uppercase() {
        byte + 32
    } else {
        byte
    }
}

fn matches(name: &str, query: &[u8]) -> bool {
    if query.is_empty() {
        return true;
    }
    let name = name.as_bytes();
    if query.len() > name.len() {
        return false;
    }
    for start in 0..=name.len() - query.len() {
        let mut equal = true;
        for offset in 0..query.len() {
            if lowercase(name[start + offset]) != lowercase(query[offset]) {
                equal = false;
                break;
            }
        }
        if equal {
            return true;
        }
    }
    false
}

/// Fill `out` with the applications matching the current search, returning how
/// many entries were written.
pub fn visible(out: &mut [usize; APP_COUNT]) -> usize {
    let state = STATE.lock();
    let query = &state.search[..state.search_len];
    let mut count = 0;
    for (index, (name, _)) in APPS.iter().enumerate() {
        if matches(name, query) {
            out[count] = index;
            count += 1;
        }
    }
    count
}

/// Selection index within the filtered list.
pub fn selection() -> usize {
    STATE.lock().selection
}

pub fn session() -> Option<usize> { STATE.lock().session }
pub fn focus_session(index: Option<usize>) { STATE.lock().session = index; }

/// Copy the current search query and its length.
pub fn search_text() -> ([u8; SEARCH_CAPACITY], usize) {
    let state = STATE.lock();
    (state.search, state.search_len)
}

/// Move the selection by `delta`, clamped to the filtered list.
pub fn move_selection(delta: isize) {
    let mut state = STATE.lock();
    let query = &state.search[..state.search_len];
    let count = APPS
        .iter()
        .filter(|(name, _)| matches(name, query))
        .count();
    if count == 0 {
        state.selection = 0;
        return;
    }
    let current = state.selection as isize;
    state.selection = (current + delta).clamp(0, count as isize - 1) as usize;
}

/// Select a filtered-list entry by index (used by mouse clicks).
pub fn set_selection(index: usize) {
    let mut state = STATE.lock();
    let query = &state.search[..state.search_len];
    let count = APPS
        .iter()
        .filter(|(name, _)| matches(name, query))
        .count();
    if count > 0 {
        state.selection = index.min(count - 1);
    }
}

/// Select an application by its entry in [`APPS`] (used by mouse clicks).
pub fn select_app(app: usize) {
    let mut state = STATE.lock();
    let query = &state.search[..state.search_len];
    let mut position = 0;
    for (index, (name, _)) in APPS.iter().enumerate() {
        if matches(name, query) {
            if index == app {
                state.selection = position;
                return;
            }
            position += 1;
        }
    }
}

/// Append a printable character to the search box.
pub fn type_char(byte: u8) -> bool {
    if !(0x20..=0x7E).contains(&byte) {
        return false;
    }
    let mut state = STATE.lock();
    let index = state.search_len;
    if index >= SEARCH_CAPACITY {
        return true;
    }
    state.search[index] = byte;
    state.search_len = index + 1;
    state.selection = 0;
    true
}

/// Remove the last search character.
pub fn backspace() -> bool {
    let mut state = STATE.lock();
    if state.search_len > 0 {
        let index = state.search_len - 1;
        state.search[index] = 0;
        state.search_len = index;
        state.selection = 0;
        true
    } else {
        false
    }
}

/// The highlighted application index, if the filtered list is not empty.
pub fn selected_app() -> Option<usize> {
    let state = STATE.lock();
    let query = &state.search[..state.search_len];
    let mut visible = [0usize; APP_COUNT];
    let mut count = 0;
    for (index, (name, _)) in APPS.iter().enumerate() {
        if matches(name, query) {
            visible[count] = index;
            count += 1;
        }
    }
    if state.selection < count {
        Some(visible[state.selection])
    } else {
        None
    }
}
