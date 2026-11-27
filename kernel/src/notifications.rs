//! Bounded in-memory notification history for the current desktop session.
use crate::{sync::SpinLock, timer};

#[derive(Clone, Copy)]
pub(crate) struct Notice {
    pub source: &'static str,
    pub second: u64,
    bytes: [u8; 96],
    len: usize,
}

impl Notice {
    const EMPTY: Self = Self {
        source: "",
        second: 0,
        bytes: [0; 96],
        len: 0,
    };
    pub fn text(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len]).unwrap_or("")
    }
}

static HISTORY: SpinLock<[Notice; 8]> = SpinLock::new([Notice::EMPTY; 8]);

pub(crate) fn post(source: &'static str, text: &str) {
    let mut notice = Notice {
        source,
        second: timer::ticks() / 100,
        ..Notice::EMPTY
    };
    let mut len = text.len().min(notice.bytes.len());
    while !text.is_char_boundary(len) {
        len -= 1;
    }
    notice.bytes[..len].copy_from_slice(&text.as_bytes()[..len]);
    notice.len = len;
    let mut history = HISTORY.lock();
    history.rotate_right(1);
    history[0] = notice;
    crate::wm::invalidate();
}

pub(crate) fn snapshot() -> [Notice; 8] {
    *HISTORY.lock()
}

pub(crate) fn count() -> usize {
    HISTORY.lock().iter().take_while(|n| !n.source.is_empty()).count()
}

pub(crate) fn dismiss() {
    let mut history = HISTORY.lock();
    history.rotate_left(1);
    history[7] = Notice::EMPTY;
}

pub(crate) fn clear() {
    HISTORY.lock().fill(Notice::EMPTY);
}
