//! Read-only ustar boot module and the file/memory calls used by ld.so.
//! No syscall exposes a host descriptor or resolves paths outside the archive.
use super::*;
use core::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone,Copy)]
struct File { bytes: &'static [u8], name: &'static str, ino: u64, pos: usize }
static mut FDS: [Option<File>; 64] = [None; 64];
const MAP_START: u64 = BASE + 0x600000;
static TRACE: [AtomicUsize;8] = [const {AtomicUsize::new(0)};8];
static mut TEXT: [Option<(&str,u64,u64,u64)>;128] = [None;128];

pub(super) fn describe(pc:u64) {
    for &(name,begin,end,offset) in unsafe{ &*core::ptr::addr_of!(TEXT) }.iter().flatten() {
        if (begin..end).contains(&pc) {
            log::serial_puts(name);log::serial_puts("+");log::serial_put_hex(pc-begin+offset);
        }
    }
}

fn lookup(path: &str) -> Option<File> {
    let tar = limine::module_bytes(2)?;
    let mut off = 0;
    while let Some(h) = tar.get(off..)?.get(..512) {
        if h[0] == 0 { break; }
        let size = core::str::from_utf8(&h[124..136]).ok()?.trim_end_matches(['\0',' ']).trim();
        let size = usize::from_str_radix(size,8).ok()?;
        let name = h[..100].split(|b| *b == 0).next()?;
        let start = off.checked_add(512)?;
        let end = start.checked_add(size)?;
        if name == path.strip_prefix('/')?.as_bytes() && matches!(h[156],0|b'0') {
            return Some(File {bytes: tar.get(start..end)?, name:core::str::from_utf8(name).ok()?, ino: start as u64, pos:0});
        }
        off = end.checked_next_multiple_of(512)?;
    }
    None
}

fn mmap(s: &mut Space, f: &interrupt::InterruptFrame) -> i64 {
    let fixed = f.r10 & 16 != 0;
    let prot = f.rdx;
    if f.rsi == 0 || prot & !7 != 0 || prot & 6 == 6 || f.r9 % PAGE as u64 != 0 || f.r10 & 3 != 2 { return -22; }
    let len = match f.rsi.checked_next_multiple_of(PAGE as u64) { Some(n) => n, None => return -12 };
    let addr = if fixed {f.rdi} else {
        let mut candidate = MAP_START;
        loop {
            let Some(stop)=candidate.checked_add(len) else {return -12};
            if stop>STACK-PAGE as u64 {return -12;}
            let start=Space::index(candidate).unwrap();
            let count=(len/PAGE as u64) as usize;
            if let Some(last)=s.ptes[start..start+count].iter().rposition(|v|*v!=0) {
                candidate+=((last+1)*PAGE) as u64;
            } else {break candidate;}
        }
    };
    let Some(end) = addr.checked_add(len) else { return -12 };
    if addr < BASE + 0x600000 || addr % PAGE as u64 != 0 || end > STACK - PAGE as u64 { return -12; }
    let source = if f.r10 & 32 == 0 {
        let Some(file) = fd(f.r8) else { return -9 };
        if prot&4!=0 {
            if let Some(slot)=unsafe{&mut *addr_of_mut!(TEXT)}.iter_mut().find(|slot|slot.is_none()) {
                *slot=Some((file.name,addr,end,f.r9));
            }
        }
        Some(file.bytes)
    } else { None };
    for p in (addr..end).step_by(PAGE) {
        let i = Space::index(p).unwrap();
        if !fixed && s.ptes[i] != 0 { return -12; }
        s.ptes[i] = 0;
        let page = s.map(p,prot & 2 != 0, prot & 4 != 0).unwrap();
        if let Some(bytes) = source {
            if let Some(offset) = f.r9.checked_add(p-addr).and_then(|n| usize::try_from(n).ok()) {
                if let Some(rest) = bytes.get(offset..) {
                    let n = PAGE.min(rest.len());
                    page[..n].copy_from_slice(&rest[..n]);
                }
            }
        }
        // Keep physical frame metadata for later mprotect, remove user access
        // for PROT_NONE so userspace cannot touch a reservation.
        if prot == 0 { s.ptes[i] &= !4; }
        unsafe { asm!("invlpg [{}]",in(reg) p); }
    }
    addr as i64
}

fn fd(n: u64) -> Option<File> {
    if n < 64 { unsafe { FDS[n as usize] } } else { None }
}

fn stat(s: &mut Space, file: File, out:u64) -> i64 {
    let mut st = [0;144];
    st[0..8].copy_from_slice(&1u64.to_le_bytes()); // st_dev
    st[8..16].copy_from_slice(&file.ino.to_le_bytes());
    st[16..24].copy_from_slice(&1u64.to_le_bytes());
    st[24..28].copy_from_slice(&0o100555u32.to_le_bytes());
    st[48..56].copy_from_slice(&(file.bytes.len() as u64).to_le_bytes());
    st[56..64].copy_from_slice(&4096u64.to_le_bytes());
    if s.write(out,&st).is_ok() {0} else {-14}
}

pub(super) fn syscall(s: &mut Space,f:&interrupt::InterruptFrame) -> i64 {
    match f.rax {
        2 | 257 => {
            let (p,flags) = if f.rax == 2 {(f.rdi,f.rsi)} else {(f.rsi,f.rdx)};
            let Some(path) = s.path(p) else { return -14 };
            if flags & 3 != 0 { return -30; }
            let Some(file) = lookup(path) else { return -2 };
            for i in 3..64 {
                unsafe { if FDS[i].is_none() {FDS[i]=Some(file); return i as i64;} }
            }
            -24
        }
        0 | 17 => {
            let Some(file) = fd(f.rdi) else {return -9};
            let pos = if f.rax == 0 {file.pos} else {f.r10 as usize};
            let bytes = file.bytes.get(pos..).unwrap_or(&[]);
            let n = bytes.len().min(f.rdx as usize);
            if s.write(f.rsi,&bytes[..n]).is_err() { return -14; }
            if f.rax == 0 { unsafe {FDS[f.rdi as usize].as_mut().unwrap().pos += n;} }
            n as i64
        }
        3 => {
            if fd(f.rdi).is_none() { return -9; }
            unsafe {FDS[f.rdi as usize]=None;} 0
        }
        4 | 5 | 262 => {
            let (file,out) = match f.rax {
                4 => (s.path(f.rdi).and_then(lookup),f.rsi),
                5 => (fd(f.rdi),f.rsi),
                _ if f.r10 & 0x1000 != 0 && s.path(f.rsi) == Some("") => (fd(f.rdi),f.rdx),
                _ => (s.path(f.rsi).and_then(lookup),f.rdx),
            };
            file.map(|v| stat(s,v,out)).unwrap_or(-2)
        }
        8 => {
            let Some(file) = fd(f.rdi) else {return -9};
            let base = match f.rdx {0=>0,1=>file.pos,2=>file.bytes.len(),_=>return -22};
            let Some(pos) = base.checked_add_signed(f.rsi as isize) else {return -22};
            if pos > i64::MAX as usize {return -22;}
            unsafe {FDS[f.rdi as usize].as_mut().unwrap().pos=pos;} pos as i64
        }
        9 => {
            let r=mmap(s,f);
            if r<0 {
                log::serial_puts("[runelf] mmap failed: len="); log::serial_put_hex(f.rsi);
                log::serial_puts(" addr="); log::serial_put_hex(f.rdi);
                log::serial_puts(" prot="); log::serial_put_hex(f.rdx);
                log::serial_puts(" flags="); log::serial_put_hex(f.r10);
                log::serial_puts("\n");
            }
            r
        },
        10 | 11 => {
            let prot = f.rdx;
            let Some(sz) = f.rsi.checked_next_multiple_of(PAGE as u64) else {return -22};
            let Some(end) = f.rdi.checked_add(sz) else {return -22};
            if f.rdi < BASE || f.rdi % PAGE as u64 != 0 || end > END || (f.rax==10 && (prot & !7 != 0 || prot & 6 == 6)) {return -22;}
            for addr in (f.rdi..end).step_by(PAGE) {
                let i = Space::index(addr).unwrap();
                if f.rax == 11 {s.ptes[i]=0;} else {
                    if s.ptes[i] & 1 == 0 {return -12;}
                    s.ptes[i] = (s.ptes[i] & 0x000f_ffff_ffff_f000) | 1 |
                        (if prot!=0 {4} else {0}) | (if prot & 2 != 0 {2} else {0}) |
                        (if prot & 4 == 0 {NX} else {0});
                }
                unsafe {asm!("invlpg [{}]",in(reg) addr);}
            }
            0
        }
        12 => (BASE + 0x400000) as i64, // fixed break; libc can use anonymous mmap
        21 => if s.path(f.rdi).and_then(lookup).is_some() {0} else {-2},
        89 if s.path(f.rdi)==Some("/proc/self/exe") => {
            let bytes = b"/app/firefox";
            let n = bytes.len().min(f.rdx as usize);
            if s.write(f.rsi,&bytes[..n]).is_ok() {n as i64} else {-14}
        }
        39 => 1,
        102 | 104 | 107 | 108 => 1000,
        228 | 229 if matches!(f.rdi,1|4|6|7) => {
            let ticks = if f.rax == 229 {1} else {timer::ticks()};
            let mut ts = [0;16];
            ts[..8].copy_from_slice(&(ticks/100).to_le_bytes());
            ts[8..].copy_from_slice(&((ticks%100)*10_000_000).to_le_bytes());
            if (f.rax == 229 && f.rsi == 0) || s.write(f.rsi,&ts).is_ok() {0} else {-14}
        }
        318 => {
            let n = (f.rsi as usize).min(256);
            for i in (0..n).step_by(16) {
                let Ok(entropy) = random() else {return -5;};
                let Some(dst) = f.rdi.checked_add(i as u64) else {return -14;};
                if s.write(dst,&entropy[..16.min(n-i)]).is_err() {return -14;}
            }
            n as i64
        }
        _ => {
            let bit=1usize<<(f.rax%64);
            if f.rax<512 && TRACE[f.rax as usize/64].fetch_or(bit,Ordering::Relaxed)&bit==0 {
                log::serial_puts("[runelf] unsupported syscall "); log::serial_put_dec(f.rax); log::serial_puts("\n");
            }
            -38
        }
    }
}
