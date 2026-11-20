//! Allocation-free x86-64 ELF load-image preparation for a future user pager.
//! Addresses are relative to the image's link base; this does not relocate,
//! map or execute instructions.
#![no_std]

/// x86-64 base page size.
pub const PAGE: usize = 4096;

fn word(bytes: &[u8], i: usize) -> u16 {
    u16::from_le_bytes(bytes[i..i + 2].try_into().unwrap())
}
fn dword(bytes: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap())
}
fn qword(bytes: &[u8], i: usize) -> u64 {
    u64::from_le_bytes(bytes[i..i + 8].try_into().unwrap())
}
fn contents<'a>(file: &'a [u8], ph: &[u8]) -> Option<&'a [u8]> {
    let start = usize::try_from(qword(ph, 8)).ok()?;
    let len = usize::try_from(qword(ph, 32)).ok()?;
    file.get(start..start.checked_add(len)?)
}

/// Validated, file-backed image. No attacker-supplied address is dereferenced.
pub struct Image<'a> {
    file: &'a [u8],
    headers: &'a [u8],
    /// Linked entry point; shared libraries may not have one.
    pub entry: u64,
    /// Dynamic linker requested by the main executable, if any.
    pub interpreter: Option<&'a str>,
    /// True for ET_DYN (load bias required).
    pub position_independent: bool,
}

impl<'a> Image<'a> {
    /// Validate the complete program header table before preparing any pages.
    pub fn parse(file: &'a [u8]) -> Result<Self, &'static str> {
        if file.len() < 64 || file[..7] != *b"\x7fELF\x02\x01\x01"
            || !matches!(word(file, 16), 2 | 3) || word(file, 18) != 62
            || dword(file, 20) != 1 || word(file, 52) != 64 || word(file, 54) != 56 {
            return Err("expected little-endian x86-64 ELF executable or shared object");
        }
        let offset = usize::try_from(qword(file, 32)).map_err(|_| "bad header offset")?;
        let count = word(file, 56) as usize;
        if count == 0 || count == 65535 { return Err("unsupported header count"); }
        let end = offset.checked_add(count * 56).ok_or("header overflow")?;
        let headers = file.get(offset..end).ok_or("truncated program header table")?;
        let mut interpreter = None;
        let entry = qword(file, 24);
        let mut entry_ok = word(file, 16) == 3 && entry == 0;
        let mut previous_end = 0;
        for ph in headers.chunks_exact(56) {
            match dword(ph, 0) {
                1 => {
                    let addr = qword(ph, 16);
                    let size = qword(ph, 40);
                    let align = qword(ph, 48);
                    let flags = dword(ph, 4);
                    let blob = contents(file, ph).ok_or("load bytes outside file")?;
                    if size < blob.len() as u64 { return Err("filesz exceeds memsz"); }
                    if size == 0 { continue; }
                    let end = addr.checked_add(size).ok_or("address overflow")?;
                    // Shared pages would require merging permissions/data. Reject
                    // those layouts rather than silently clobbering the first load.
                    if addr < previous_end || end > 1 << 47 || flags & 3 == 3 {
                        return Err("overlapping, non-user or writable-executable load");
                    }
                    let off = qword(ph, 8);
                    if addr % PAGE as u64 != off % PAGE as u64 || (align > 1
                        && (!align.is_power_of_two() || addr % align != off % align)) {
                        return Err("invalid load alignment");
                    }
                    previous_end = end.next_multiple_of(PAGE as u64);
                    entry_ok |= flags & 1 != 0 && entry >= addr && entry - addr < blob.len() as u64;
                }
                3 => {
                    let data = contents(file, ph).ok_or("interpreter outside file")?;
                    let path = data.strip_suffix(&[0]).ok_or("unterminated interpreter")?;
                    if interpreter.is_some() || path.first() != Some(&b'/') || path.contains(&0) {
                        return Err("invalid interpreter path");
                    }
                    interpreter = Some(core::str::from_utf8(path).map_err(|_| "non-UTF8 interpreter")?);
                }
                _ => {}
            }
        }
        if previous_end == 0 || !entry_ok { return Err("missing load image or invalid entry"); }
        Ok(Self { file, headers, entry, interpreter, position_independent: word(file, 16) == 3 })
    }

    /// Linked address and dimensions of the mapped program header table.
    /// Linux dynamic linkers consume these via AT_PHDR, AT_PHENT and AT_PHNUM.
    pub fn program_headers(&self) -> Option<(u64, u64, u64)> {
        let off = qword(self.file, 32);
        self.headers.chunks_exact(56).find_map(|ph| {
            let delta = off.checked_sub(qword(ph, 8))?;
            (dword(ph, 0) == 1 && delta.checked_add(self.headers.len() as u64)? <= qword(ph, 32))
                .then(|| (qword(ph, 16) + delta, 56, (self.headers.len() / 56) as u64))
        })
    }

    /// Nonempty PT_LOAD segments in ascending link address order.
    pub fn segments(&self) -> impl Iterator<Item = Segment<'a>> + '_ {
        self.headers.chunks_exact(56)
            .filter(|h| dword(h, 0) == 1 && qword(h, 40) != 0)
            .map(|h| Segment {
                file: contents(self.file, h).unwrap(),
                addr: qword(h, 16), size: qword(h, 40), flags: dword(h, 4),
            })
    }
}

/// One validated segment, with its ELF permission flags (R=4, W=2, X=1).
pub struct Segment<'a> {
    file: &'a [u8],
    addr: u64,
    size: u64,
    /// Requested memory permissions.
    pub flags: u32,
}

impl Segment<'_> {
    /// Page-aligned interval to reserve (exclusive end).
    pub fn range(&self) -> core::ops::Range<u64> {
        (self.addr & !(PAGE as u64 - 1))..(self.addr + self.size).next_multiple_of(PAGE as u64)
    }

    /// Initialize one page from the file, zeroing BSS and padding, with no
    /// assumption that the segment starts at a page boundary.
    pub fn load_page(&self, addr: u64, page: &mut [u8; PAGE]) -> Result<(), &'static str> {
        if addr % PAGE as u64 != 0 || !self.range().contains(&addr) {
            return Err("not a segment page");
        }
        page.fill(0);
        let start = addr.max(self.addr);
        let end = (addr + PAGE as u64).min(self.addr + self.file.len() as u64);
        if end > start {
            page[(start-addr) as usize..(end-addr) as usize]
                .copy_from_slice(&self.file[(start-self.addr) as usize..(end-self.addr) as usize]);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_truncated_and_out_of_bounds_images() {
        let mut elf = [0u8; 120];
        elf[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        elf[16] = 2; // ET_EXEC
        elf[18] = 62;
        elf[20] = 1;
        elf[24..32].copy_from_slice(&0x400040u64.to_le_bytes());
        elf[32] = 64;
        elf[52] = 64;
        elf[54] = 56;
        elf[56] = 1;
        elf[64] = 1; // PT_LOAD containing header bytes
        elf[68] = 5;
        elf[80..88].copy_from_slice(&0x400000u64.to_le_bytes());
        elf[96] = 120;
        elf[104] = 120;
        assert!(Image::parse(&elf).is_ok());
        for end in 0..elf.len() {
            assert!(Image::parse(&elf[..end]).is_err());
        }
        elf[96] = 121;
        assert!(Image::parse(&elf).is_err());
        elf[96] = 120;
        elf[68] = 7;
        assert!(Image::parse(&elf).is_err());
    }

    #[test]
    fn partial_pages_do_not_leak_file_padding_or_stale_bss() {
        let s = Segment { file: b"abcd", addr: 4094, size: 8200, flags: 6 };
        let mut page = [0xff; PAGE];
        s.load_page(0, &mut page).unwrap();
        assert!(page[..4094].iter().all(|b| *b == 0));
        assert_eq!(&page[4094..], b"ab");
        s.load_page(4096, &mut page).unwrap();
        assert_eq!(&page[..2], b"cd");
        assert!(page[2..].iter().all(|b| *b == 0));
        s.load_page(8192, &mut page).unwrap();
        assert_eq!(page, [0; PAGE]);
        assert!(s.load_page(16384, &mut page).is_err());
    }
}
