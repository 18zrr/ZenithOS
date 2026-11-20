//! Small thread group for Linux startup. Context switches only at a user
//! trap boundary; every thread has its own general registers, FPU and TLS.
use super::*;
use crate::interrupt::UserFrame;

#[derive(Clone,Copy)]
struct Saved { frame: UserFrame, fx: [u8;512], fs:u64 }
#[derive(Clone,Copy)]
struct Thread { saved: Option<Saved>, tid:u32, clear:u64, wait:u64 }
const FREE: Thread = Thread {saved: None,tid:0,clear:0,wait:0};
static mut GROUP: [Thread;16] = [FREE;16];
static mut CURRENT: usize = 0;
static mut ID: u32 = 1;

unsafe fn group() -> &'static mut [Thread;16] {
    &mut *addr_of_mut!(GROUP)
}

pub(super) fn schedule(u: &mut UserFrame,fx:&mut [u8;512]) {
    unsafe {
        let g = group();
        let cur = CURRENT;
        let next = (1..g.len()).map(|d| (cur+d)%g.len()).find(|&i|g[i].saved.is_some() && g[i].wait==0);
        if let Some(i) = next {
            if g[cur].tid!=0 {
                g[cur].saved = Some(Saved {frame:*u,fx:*fx,fs:rdmsr(0xc0000100)});
            }
            let task = g[i].saved.take().unwrap();
            *u=task.frame; *fx=task.fx;
            wrmsr(0xc0000100,task.fs);
            CURRENT=i;
        } else if g[cur].wait!=0 || g[cur].tid==0 {
            log::serial_puts("[runelf] no runnable threads\n");
            native_finish(513);
        }
    }
}

pub(super) fn init() {
    unsafe { group()[0].tid=1; }
}

pub(super) fn retry_after(key:u64,u:&mut UserFrame,fx:&mut [u8;512]) {
    // Resume at the SYSCALL instruction. Arguments remain in their Linux ABI registers.
    unsafe {group()[CURRENT].wait=key;}
    u.regs.rip-=2;
    schedule(u,fx);
}

pub(super) fn wake(key:u64) {
    for t in unsafe{group()} {
        if t.wait==key {t.wait=0;}
    }
}

pub(super) fn call(s:&mut Space,u:&mut UserFrame,fx:&mut [u8;512]) {
    unsafe {
        let g=group();
        let cur=CURRENT;
        let f=&mut u.regs;
        let result:i64 = match f.rax {
            186 => g[cur].tid as i64,
            218 => {g[cur].clear=f.rdi; g[cur].tid as i64}
            231 => native_finish(f.rdi&255),
            60 => {
                let ptr=g[cur].clear;
                if ptr!=0 && s.write(ptr,&0u32.to_le_bytes()).is_ok() {
                    for t in g.iter_mut() {if t.wait==ptr {t.wait=0;}}
                }
                g[cur]=FREE;
                if g.iter().all(|t| t.tid==0) {native_finish(f.rdi&255);}
                schedule(u,fx);
                return;
            }
            56 => {
                // Support pthread-style shared VM/files/signal handlers only.
                let flags=f.rdi;
                let required=0x100|0x200|0x400|0x800|0x10000;
                if flags & required!=required || flags & !(required|0x40000|0x80000|0x100000|0x200000|0x400000|0x1000000)!=0 {
                    -22
                } else if !f.rsi.checked_sub(8).is_some_and(|p|s.valid(p,8,true))
                    || (flags&0x80000!=0 && !s.valid(f.r8,8,true))
                    || (flags&0x100000!=0 && !s.valid(f.rdx,4,true))
                    || (flags&0x1000000!=0 && !s.valid(f.r10,4,true)) { -14 }
                else if let Some(slot)=g.iter_mut().find(|t|t.tid==0) {
                    ID+=1;
                    let tid=ID;
                    if flags&0x100000!=0 {s.write(f.rdx,&tid.to_ne_bytes()).unwrap();}
                    if flags&0x1000000!=0 {s.write(f.r10,&tid.to_ne_bytes()).unwrap();}
                    let clear=if flags&0x200000!=0 {f.r10} else {0};
                    let fs=if flags&0x80000!=0 {f.r8} else {rdmsr(0xc0000100)};
                    let sp=f.rsi;
                    let mut frame=*u;
                    frame.regs.rax=0; frame.sp=sp;
                    *slot=Thread {tid,clear,wait:0,saved:Some(Saved{frame,fx:*fx,fs})};
                    tid as i64
                } else {-11}
            }
            202 => {
                let op=f.rsi&127;
                if f.rdi%4!=0 { -22 } else if let Some(value)=s.data(f.rdi,4) {
                    match op {
                        0|9 if value!=(f.rdx as u32).to_le_bytes() => -11,
                        0|9 if f.r10!=0 || (op==9 && f.r9!=0xffff_ffff) => -38,
                        0|9 => {
                            g[cur].wait=f.rdi;
                            f.rax=0;
                            schedule(u,fx);
                            return;
                        }
                        1|10 if op==1 || f.r9==0xffff_ffff => {
                            let mut woke=0;
                            for t in g.iter_mut().filter(|t|t.wait==f.rdi).take(f.rdx as usize) {
                                t.wait=0; woke+=1;
                            } woke
                        }
                        _=>-38,
                    }
                } else {-14}
            }
            _=>-38,
        };
        u.regs.rax=result as u64;
    }
}
