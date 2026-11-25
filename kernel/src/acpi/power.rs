//! Fixed ACPI hardware using System I/O registers and a literal DSDT _S5
//! package. Unsupported AML methods and hardware-reduced platforms fall back.
use crate::{diagnostics, io, limine, sync::SpinLock};

#[derive(Clone, Copy, Default)]
struct Power {
    a: u16,
    b: u16,
    types: [u16; 2],
    reset: u16,
    value: u8,
    smi: u16,
    enable: u8,
}
static POWER: SpinLock<Option<Power>> = SpinLock::new(None);

fn word(buf: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(buf.get(i..i + 4)?.try_into().ok()?))
}
fn quad(buf: &[u8], i: usize) -> Option<u64> {
    Some(u64::from_le_bytes(buf.get(i..i + 8)?.try_into().ok()?))
}
fn bytes(p: u64, n: usize) -> Option<&'static [u8]> {
    // Both the legacy identity map and base-revision-zero Limine HHDM
    // cover the low 4 GiB. Decline tables outside this supported range.
    if p < 4096 || p.checked_add(n as u64)? > 1 << 32 {
        return None;
    }
    Some(unsafe { core::slice::from_raw_parts((limine::hhdm_offset() + p) as *const u8, n) })
}
fn table(p: u64, sig: &[u8; 4]) -> Option<&'static [u8]> {
    let header = bytes(p, 36)?;
    if &header[..4] != sig {
        return None;
    }
    let len = word(header, 4)? as usize;
    if !(36..=1024 * 1024).contains(&len) {
        return None;
    }
    let data = bytes(p, len)?;
    (data.iter().fold(0u8, |s, b| s.wrapping_add(*b)) == 0).then_some(data)
}
fn integer(data: &mut &[u8]) -> Option<u16> {
    let (&op, rest) = data.split_first()?;
    let (v, used) = match op {
        0 | 1 => (op, 0),
        0x0a => (*rest.first()?, 1),
        _ => return None,
    };
    *data = &rest[used..];
    (v < 8).then_some((v as u16) << 10)
}
fn sleep_types(dsdt: &[u8]) -> Option<[u16; 2]> {
    for (i, s) in dsdt.windows(5).enumerate() {
        if s != b"_S5_\x12"
            || !(i >= 1 && dsdt[i - 1] == 8 || i >= 2 && &dsdt[i - 2..i] == b"\x08\\")
        {
            continue;
        }
        let pkg = dsdt.get(i + 5..)?;
        let lead = *pkg.first()?;
        let follow = (lead >> 6) as usize;
        let mut len = (lead & if follow == 0 { 63 } else { 15 }) as usize;
        for j in 0..follow {
            len |= (*pkg.get(1 + j)? as usize) << (4 + 8 * j);
        }
        let body = pkg.get(follow + 1..len)?;
        let (&count, mut args) = body.split_first()?;
        if count >= 2 {
            return Some([integer(&mut args)?, integer(&mut args)?]);
        }
    }
    None
}
pub(super) fn discover(rsdp: usize, rev: u8, rsdt: u32) {
    let xsdt = if rev >= 2 && limine::is_active() {
        let ext = unsafe { core::slice::from_raw_parts(rsdp as *const u8, 36) };
        if word(ext, 20) == Some(36) && ext.iter().fold(0u8, |a, b| a.wrapping_add(*b)) == 0 {
            quad(ext, 24).and_then(|p| table(p, b"XSDT"))
        } else {
            None
        }
    } else {
        None
    };
    let Some((root, step)) = xsdt
        .map(|t| (t, 8))
        .or_else(|| table(rsdt.into(), b"RSDT").map(|t| (t, 4)))
    else {
        return;
    };
    let Some(fadt) = root[36..].chunks_exact(step).find_map(|v| {
        let p = if step == 8 {
            quad(v, 0)?
        } else {
            word(v, 0)? as u64
        };
        table(p, b"FACP")
    }) else {
        return;
    };
    let mut p = Power::default();
    let flags = word(fadt, 112).unwrap_or(0);
    if flags & (1 << 10) != 0 && fadt.get(116..119) == Some(&[1, 8, 0]) && fadt.len() >= 129 {
        p.reset = quad(fadt, 120)
            .and_then(|v| u16::try_from(v).ok())
            .unwrap_or(0);
        p.value = fadt[128];
    }
    if flags & (1 << 20) == 0 && fadt.get(89) == Some(&2) {
        let dsdt = quad(fadt, 140)
            .and_then(|v| table(v, b"DSDT"))
            .or_else(|| table(word(fadt, 40)? as u64, b"DSDT"));
        if let Some(types) = dsdt.and_then(|d| sleep_types(&d[36..])) {
            p.types = types;
            p.a = word(fadt, 64).and_then(|v| v.try_into().ok()).unwrap_or(0);
            p.b = word(fadt, 68).and_then(|v| v.try_into().ok()).unwrap_or(0);
            p.smi = word(fadt, 48).and_then(|v| v.try_into().ok()).unwrap_or(0);
            p.enable = fadt[52];
        }
    }
    if p.a != 0 {
        diagnostics::serial_puts("[acpi] firmware S5 power control ready\n");
    }
    *POWER.lock() = Some(p);
}
pub(super) unsafe fn off() {
    let Some(p) = *POWER.lock() else { return };
    if p.a == 0 {
        return;
    }
    if io::inw(p.a) & 1 == 0 && p.smi != 0 && p.enable != 0 {
        io::outb(p.smi, p.enable);
        for _ in 0..100_000 {
            if io::inw(p.a) & 1 != 0 {
                break;
            }
        }
        if io::inw(p.a) & 1 == 0 {
            return;
        }
    }
    for (port, t) in [(p.a, p.types[0]), (p.b, p.types[1])] {
        if port != 0 {
            io::outw(port, (io::inw(port) & !0x3c00) | t | (1 << 13));
        }
    }
}
pub(super) unsafe fn reset() {
    if let Some(p) = *POWER.lock() {
        if p.reset != 0 {
            io::outb(p.reset, p.value);
        }
    }
}
