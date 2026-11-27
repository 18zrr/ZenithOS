//! Session-local text clipboard shared by the editor and shell.
use crate::sync::SpinLock;
use alloc::string::String;

static TEXT: SpinLock<String> = SpinLock::new(String::new());

/// Both producers are bounded ASCII text fields (4 KiB or less).
pub(crate) fn set(text: &str) {
    let mut current = TEXT.lock();
    current.clear();
    current.push_str(text);
}

pub(crate) fn text() -> String {
    TEXT.lock().clone()
}
