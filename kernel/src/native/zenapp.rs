//! Zenith application ABI v1. Only dispatched for an explicitly selected
//! `zenapp` boot module, never for the Linux compatibility runner.
use super::{native_finish, Space};
use crate::{allocator, diagnostics, interrupt::InterruptFrame, timer};

pub(super) fn dispatch(space: &mut Space, frame: &InterruptFrame) -> i64 {
    match frame.rax {
        0 => 1, // ABI version
        1 => unsafe { native_finish(frame.rdi & 255) },
        2 => {
            if frame.rsi > 4096 {
                return -22;
            }
            let Some(bytes) = space.data(frame.rdi, frame.rsi as usize) else {
                return -14;
            };
            for &byte in bytes {
                diagnostics::putc(byte);
            }
            bytes.len() as i64
        }
        3 => {
            let release = env!("ZENITH_RELEASE").as_bytes();
            if frame.rsi < release.len() as u64 {
                return -28;
            }
            if space.write(frame.rdi, release).is_err() {
                return -14;
            }
            release.len() as i64
        }
        4 => timer::ticks() as i64,
        5 => allocator::used() as i64,
        6 => {
            if frame.rsi == 0 || frame.rsi > 96 {
                return -22;
            }
            let Some(bytes) = space.data(frame.rdi, frame.rsi as usize) else {
                return -14;
            };
            if !bytes.iter().all(|b| (32..=126).contains(b)) {
                return -22;
            }
            crate::notifications::post("Application", core::str::from_utf8(bytes).unwrap());
            0
        }
        _ => -38,
    }
}
