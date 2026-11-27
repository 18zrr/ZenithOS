//! Opt-in, one-shot Linux/x86-64 process bring-up. Each guest virtual page
//! has dedicated backing; supervisor mappings are never accessible at CPL3.
use core::arch::{asm, global_asm};
use core::ptr::addr_of_mut;
use crate::{diagnostics as log, interrupt, limine, timer};
use zenith_elf::{Image, PAGE};
mod files;
mod threads;
mod pipes;
mod zenapp;
use core::sync::atomic::{AtomicBool, Ordering};

static ZENAPP: AtomicBool = AtomicBool::new(false);

global_asm!(include_str!("arch/x86_64/user.s"), options(att_syntax));

extern "C" {
    fn native_gdt();
    fn native_enter(entry: u64, sp: u64, root: u64) -> u64;
    fn native_syscall();
    fn native_finish(code: u64) -> !;
}

const BASE: u64 = 0x400000;
const REGIONS: usize = 256;
const END: u64 = BASE + (REGIONS as u64) * 0x200000;
const LOADER: u64 = BASE + 0x200000;
const STACK: u64 = END - 0x100000;
const NX: u64 = 1 << 63;

#[repr(C, align(4096))]
struct Space {
    root: [u64; 512],
    pdpt: [u64; 512],
    pd: [u64; 512],
    ptes: [u64; REGIONS * 512],
    bytes: [[u8; PAGE]; REGIONS * 512],
}
static mut SPACE: Space = Space {
    root: [0; 512], pdpt: [0; 512], pd: [0; 512], ptes: [0; REGIONS * 512],
    bytes: [[0; PAGE]; REGIONS * 512],
};
static mut DEADLINE: u64 = 0;
const EFER: u32 = 0xc0000080;

unsafe fn rdmsr(msr: u32) -> u64 {
    let (lo, hi): (u32, u32);
    asm!("rdmsr", in("ecx") msr, out("eax") lo, out("edx") hi);
    u64::from(lo) | (u64::from(hi) << 32)
}
unsafe fn wrmsr(msr: u32, val: u64) {
    asm!("wrmsr", in("ecx") msr, in("eax") val as u32, in("edx") (val >> 32) as u32);
}

pub fn init_segments() {
    unsafe { native_gdt() };
}

impl Space {
    fn setup(&mut self) -> Result<(), &'static str> {
        let (cr3, cr4): (u64, u64);
        unsafe { asm!("mov {}, cr3", "mov {}, cr4", out(reg) cr3, out(reg) cr4) };
        if cr4 & (1 << 12) != 0 { return Err("only four-level paging supported"); }
        let parent = ((cr3 & 0x000f_ffff_ffff_f000) + limine::hhdm_offset()) as *const u64;
        for i in 256..512 {
            self.root[i] = unsafe { *parent.add(i) } & !4;
        }
        self.root[0] = limine::physical(self.pdpt.as_ptr() as usize) | 7;
        self.pdpt[0] = limine::physical(self.pd.as_ptr() as usize) | 7;
        for i in 0..REGIONS {
            self.pd[(BASE >> 21) as usize + i] = limine::physical(&self.ptes[i*512] as *const _ as usize) | 7;
        }
        Ok(())
    }

    fn index(addr: u64) -> Option<usize> {
        (BASE..END).contains(&addr).then(|| ((addr - BASE) as usize) / PAGE)
    }

    fn map(&mut self, addr: u64, writable: bool, exec: bool) -> Result<&mut [u8; PAGE], &'static str> {
        let i = Self::index(addr).ok_or("load outside address space")?;
        if addr % PAGE as u64 != 0 || self.ptes[i] != 0 || writable && exec {
            return Err("invalid load mapping");
        }
        self.ptes[i] = limine::physical(self.bytes[i].as_ptr() as usize)
            | 5 | if writable {2} else {0} | if exec {0} else {NX};
        self.bytes[i].fill(0);
        Ok(&mut self.bytes[i])
    }

    fn image(&mut self, img: &Image, bias: u64) -> Result<(), &'static str> {
        for seg in img.segments() {
            for addr in seg.range().step_by(PAGE) {
                let address = bias.checked_add(addr).ok_or("load address overflow")?;
                let page = self.map(address, seg.flags & 2 != 0, seg.flags & 1 != 0)?;
                seg.load_page(addr, page)?;
            }
        }
        Ok(())
    }

    fn valid(&self, ptr: u64, n: usize, write: bool) -> bool {
        if n == 0 { return true; }
        let Some(first) = Self::index(ptr) else { return false };
        let Some(last) = ptr.checked_add(n as u64 - 1).and_then(Self::index) else { return false };
        let mask = if write {7} else {5};
        self.ptes[first..=last].iter().all(|v| v & mask == mask)
    }

    fn data(&self, ptr: u64, n: usize) -> Option<&[u8]> {
        self.valid(ptr,n,false).then(|| if n == 0 { &[] } else {
            unsafe { core::slice::from_raw_parts(self.bytes.as_ptr().cast::<u8>().add((ptr-BASE) as usize), n) }
        })
    }

    fn path(&self, ptr: u64) -> Option<&str> {
        let len = (0..4096).find(|&n| ptr.checked_add(n).and_then(|p| self.data(p,1)) == Some(&[0][..]))?;
        core::str::from_utf8(self.data(ptr,len as usize)?).ok()
    }

    fn write(&mut self, ptr: u64, bytes: &[u8]) -> Result<(), &'static str> {
        if !self.valid(ptr,bytes.len(),true) { return Err("unmapped output buffer"); }
        if !bytes.is_empty() {
            unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(),
                self.bytes.as_mut_ptr().cast::<u8>().add((ptr-BASE) as usize),bytes.len()) };
        }
        Ok(())
    }
}

fn random() -> Result<[u8; 16], &'static str> {
    use core::arch::x86_64::{__cpuid, _rdrand64_step};
    if __cpuid(1).ecx & (1<<30) == 0 { return Err("AT_RANDOM needs RDRAND (QEMU -cpu max)"); }
    let mut out = [0;16];
    for chunk in out.chunks_exact_mut(8) {
        let mut word = 0;
        if !(0..10).any(|_| unsafe { _rdrand64_step(&mut word) == 1 }) { return Err("RDRAND failed"); }
        chunk.copy_from_slice(&word.to_ne_bytes());
    }
    Ok(out)
}

// End the mutable borrow of Space before entering user mode. The syscall
// dispatcher then has exclusive access until native_enter returns.
fn prepare(zenapp: bool) -> Result<(u64,u64,u64), &'static str> {
    let img = Image::parse(limine::module_bytes(0).ok_or("missing program module")?)?;
    if zenapp && (img.interpreter.is_some() || img.position_independent || img.entry == 0) {
        return Err("Zenith ABI v1 requires a static ET_EXEC image");
    }
    let bias = if img.position_independent { BASE } else {0};
    let space = unsafe { &mut *addr_of_mut!(SPACE) };
    space.setup()?;
    space.image(&img,bias)?;
    let (entry, ld_base) = if img.interpreter.is_some() {
        let ld = Image::parse(limine::module_bytes(1).ok_or("missing interpreter module")?)?;
        if !ld.position_independent || ld.interpreter.is_some() || ld.entry == 0 {
            return Err("invalid interpreter");
        }
        space.image(&ld, LOADER)?;
        (LOADER + ld.entry, LOADER)
    } else { (bias + img.entry, 0) };
    for addr in (STACK..END).step_by(PAGE) { space.map(addr,true,false)?; }
    if zenapp {
        // A SysV-aligned stack for a freestanding C entry function. There
        // is no caller: applications terminate through the exit syscall.
        return Ok((entry, END - 8, limine::physical(space.root.as_ptr() as usize)));
    }
    let entropy = END - 16;
    space.write(entropy,&random()?)?;
    let (phdr, phent, phnum) = img.program_headers().ok_or("unmapped PHDR")?;
    let argv: &[&str] = if limine::cmdline().split_ascii_whitespace().any(|s| s=="headless-page") {
        &["/app/firefox","--headless","--no-remote","--profile","/tmp/profile",
          "--screenshot","/tmp/page.png","file:///app/page.html"]
    } else { &["/app/firefox","--version"] };
    let mut cursor=entropy;
    let mut words=alloc::vec![argv.len() as u64];
    for group in [argv, &["LD_LIBRARY_PATH=/lib:/app","HOME=/home/zenith","TMPDIR=/tmp"][..]] {
        for text in group {
            cursor-=text.len() as u64+1;
            space.write(cursor,text.as_bytes())?;
        // Stack pages started zeroed, including each string terminator.
            words.push(cursor);
        }
        words.push(0);
    }
    let name=words[1];
    words.extend_from_slice(&[3,bias+phdr,4,phent,5,phnum,6,4096,7,ld_base,
        9,bias+img.entry,11,1000,12,1000,13,1000,14,1000,17,100,23,0,25,entropy,31,name,0,0]);
    let sp = (cursor - words.len() as u64 * 8) & !15;
    for (i,word) in words.iter().enumerate() { space.write(sp + 8*i as u64,&word.to_le_bytes())?; }
    Ok((entry,sp,limine::physical(space.root.as_ptr() as usize)))
}

pub fn boot(zenapp: bool) {
    let (entry,sp,root) = match prepare(zenapp) {
        Ok(state) => state,
        Err(e) => { log::serial_puts("[runelf] "); log::serial_puts(e); log::serial_puts("\n"); return; }
    };
    let status = interrupt::without_interrupts(|| unsafe {
        let features = core::arch::x86_64::__cpuid(0x80000001).edx;
        if features & ((1<<20)|(1<<11)) != ((1<<20)|(1<<11)) { return 256; }
        let fs = rdmsr(0xc0000100);
        let gs = rdmsr(0xc0000101);
        let saved_msrs = [EFER, 0xc0000081, 0xc0000082, 0xc0000084].map(|id| (id, rdmsr(id)));
        wrmsr(0xc0000100,0);
        wrmsr(0xc0000101,0);
        wrmsr(EFER,rdmsr(EFER) | (1<<11) | 1);
        wrmsr(0xc0000081, (8u64<<32) | (16u64<<48));
        wrmsr(0xc0000082, native_syscall as *const () as u64);
        wrmsr(0xc0000084, 0x47700);
        DEADLINE = timer::ticks() + 9000;
        threads::init();
        ZENAPP.store(zenapp, Ordering::Relaxed);
        let cr4:u64;
        asm!("mov {}, cr4",out(reg) cr4);
        // No global TLB aliases, user FSGSBASE or extended AVX state in this runner.
        asm!("mov cr4, {}",in(reg) cr4 & !((1<<7)|(1<<16)|(1<<18)));
        let result = native_enter(entry,sp,root);
        ZENAPP.store(false, Ordering::Relaxed);
        asm!("mov cr4, {}",in(reg) cr4);
        wrmsr(0xc0000100,fs);
        wrmsr(0xc0000101,gs);
        for (id, value) in saved_msrs { wrmsr(id, value); }
        result
    });
    log::serial_puts("[runelf] status=");
    log::serial_put_dec(status);
    log::serial_puts("\n");
}

pub fn trap(user: &mut interrupt::UserFrame, fx: &mut [u8;512]) -> bool {
    let f = &mut user.regs;
    if f.vector < 32 {
        log::serial_puts("[runelf] user fault vector="); log::serial_put_dec(f.vector);
        log::serial_puts(" rip="); log::serial_put_hex(f.rip);
        log::serial_puts(" ");files::describe(f.rip);
        log::serial_puts(" address="); log::serial_put_hex(crate::page_table::fault_address() as u64);
        log::serial_puts("\n");
        unsafe { native_finish(256 + f.vector) };
    }
    if f.vector == 32 && timer::ticks() >= unsafe { DEADLINE } {
        unsafe { crate::io::outb(0x20,0x20); native_finish(512) }
    }
    if f.vector == 32 {
        timer::tick();
        unsafe {crate::io::outb(0x20,0x20);}
        threads::schedule(user,fx);
        return true;
    }
    if f.vector != 256 { return false; }
    let s = unsafe { &mut *addr_of_mut!(SPACE) };
    if ZENAPP.load(Ordering::Relaxed) {
        f.rax = zenapp::dispatch(s, f) as u64;
        return true;
    }
    if matches!(f.rax,56|60|186|202|218|231) {
        threads::call(s,user,fx);
        return true;
    }
    if pipes::call(s,user,fx) { return true; }
    let f=&mut user.regs;
    let emit = |ptr, n: u64| -> i64 {
        if n > 65536 { return -22; }
        if let Some(data) = s.data(ptr,n as usize) {
            for &b in data { log::putc(b); }
            n as i64
        } else { -14 }
    };
    let result = match f.rax {
        1 if f.rdi == 1 || f.rdi == 2 => emit(f.rsi,f.rdx),
        20 if f.rdi == 1 || f.rdi == 2 => {
            let mut written = 0;
            if f.rdx > 16 { -22 } else if let Some(iov) = s.data(f.rsi, 16*f.rdx as usize) {
                for v in iov.chunks_exact(16) {
                    let n = emit(u64::from_le_bytes(v[..8].try_into().unwrap()),
                                 u64::from_le_bytes(v[8..].try_into().unwrap()));
                    if n < 0 { written = n; break; }
                    written += n;
                }
                written
            } else { -14 }
        }
        158 if f.rdi == 0x1002 && (BASE..END).contains(&f.rsi) => {
            unsafe { wrmsr(0xc0000100,f.rsi); } 0
        }
        _ => files::syscall(s,f),
    };
    f.rax = result as u64;
    true
}
