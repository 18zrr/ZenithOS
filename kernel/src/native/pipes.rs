//! Bounded in-process pipes for Firefox worker threads.
use super::*;
use crate::interrupt::UserFrame;

#[derive(Clone, Copy)]
struct Pipe {
    bytes:[u8;4096], start:usize, len:usize, reader:bool, writer:bool, nonblock:bool,
}
const EMPTY:Pipe=Pipe {bytes:[0;4096],start:0,len:0,reader:false,writer:false,nonblock:false};
static mut PIPES:[Pipe;8]=[EMPTY;8];
const FIRST:u64=128;
// Outside the user address range, so pipe wait keys never alias a futex.
fn wait_key(fd:u64)->u64 {u64::MAX-fd}

pub(super) fn call(s:&mut Space,u:&mut UserFrame,fx:&mut [u8;512])->bool {
    let f=&mut u.regs;
    let pipes=unsafe{&mut *addr_of_mut!(PIPES)};
    if f.rax==22 || f.rax==293 {
        let flags=if f.rax==22 {0} else {f.rsi};
        let res:i64=if flags & !(0x800|0x80000)!=0 {-22} else if let Some(i)=pipes.iter().position(|p|!p.reader && !p.writer) {
            let rd=(FIRST + i as u64*2) as u32;
            let pair=(rd as u64)|((rd as u64+1)<<32);
            if s.write(f.rdi,&pair.to_le_bytes()).is_ok() {
                pipes[i]=Pipe{reader:true,writer:true,nonblock:flags&0x800!=0,..EMPTY}; 0
            } else {-14}
        } else {-24};
        f.rax=res as u64; return true;
    }
    let fd=f.rdi;
    if !(FIRST..FIRST+2*pipes.len() as u64).contains(&fd) {return false;}
    let p=&mut pipes[((fd-FIRST)/2) as usize];
    let write=fd%2==1;
    let peer=fd^1;
    let n=(f.rdx as usize).min(p.bytes.len());
    let res:i64=if (write&&!p.writer)||(!write&&!p.reader) {-9} else {match f.rax {
        3=>{
            if write {p.writer=false;} else {p.reader=false;}
            threads::wake(wait_key(peer)); 0
        }
        0 if !write=>{
            let n=n.min(p.len);
            if n==0 && p.writer && f.rdx!=0 {
                if p.nonblock {-11} else {threads::retry_after(wait_key(fd),u,fx);return true;}
            } else if !s.valid(f.rsi,n,true) {-14} else {
                for i in 0..n {
                    s.write(f.rsi+i as u64,&[p.bytes[p.start]]).unwrap();
                    p.start=(p.start+1)%p.bytes.len();
                }
                p.len-=n;threads::wake(wait_key(peer));n as i64
            }
        }
        1 if write=>{
            let n=n.min(p.bytes.len()-p.len);
            if !p.reader {-32} else if n==0 && f.rdx!=0 {
                if p.nonblock {-11} else {threads::retry_after(wait_key(fd),u,fx);return true;}
            } else if let Some(bytes)=s.data(f.rsi,n) {
                for &b in bytes {p.bytes[(p.start+p.len)%p.bytes.len()]=b;p.len+=1;}
                threads::wake(wait_key(peer)); n as i64
            } else {-14}
        }
        _=>-9,
    }};
    f.rax=res as u64;
    true
}
