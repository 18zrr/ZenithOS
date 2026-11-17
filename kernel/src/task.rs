//! Cooperative task scheduler with per-task kernel stacks.

use crate::diagnostics;
use crate::keyboard;
use crate::sync::SpinLock;
use crate::timer;

const MAX_TASKS: usize = 3;
const STACK_SIZE: usize = 16 * 1024;

#[repr(align(16))]
struct AlignedStack([u8; STACK_SIZE]);

#[derive(Clone, Copy, PartialEq)]
enum TaskState {
    Ready,
    Terminated,
}

struct Task {
    func: fn(),
    state: TaskState,
    stack: AlignedStack,
}

struct TaskManager {
    tasks: [Task; MAX_TASKS],
    current: usize,
    started: bool,
}

extern "C" {
    fn switch_context(old_rsp: *mut u64, new_rsp: u64);
}

static mut RSPS: [u64; MAX_TASKS] = [0; MAX_TASKS];

static TASKS: SpinLock<TaskManager> = SpinLock::new(TaskManager {
    tasks: [
        Task {
            func: shell_task,
            state: TaskState::Ready,
            stack: AlignedStack([0; STACK_SIZE]),
        },
        Task {
            func: heartbeat_task,
            state: TaskState::Ready,
            stack: AlignedStack([0; STACK_SIZE]),
        },
        Task {
            func: idle_task,
            state: TaskState::Ready,
            stack: AlignedStack([0; STACK_SIZE]),
        },
    ],
    current: 0,
    started: false,
});

/// Prepare each task's initial context frame.
pub fn init() {
    let mut manager = TASKS.lock();
    for index in 0..MAX_TASKS {
        let top = manager.tasks[index].stack.0.as_ptr() as usize + STACK_SIZE;
        // Six saved registers plus the return address occupy 56 bytes. An extra
        // 8 bytes makes RSP 8 (mod 16) after the return into task_trampoline,
        // as required by the SysV ABI for aligned SSE stores in task functions.
        let frame = top - 64;
        unsafe {
            for offset in (0..48).step_by(8) {
                *((frame + offset) as *mut u64) = 0;
            }
            *((frame + 48) as *mut u64) = task_trampoline as *const () as usize as u64;
        }
        unsafe { RSPS[index] = frame as u64 };
    }
    manager.started = true;
}

/// Start the first task. This call never returns.
pub fn start() -> ! {
    let mut discarded_rsp: u64 = 0;
    let first_rsp = unsafe { RSPS[0] };
    unsafe { switch_context(&mut discarded_rsp, first_rsp) }
    loop {
        unsafe { core::arch::asm!("hlt", options(nomem, nostack)) }
    }
}

/// Voluntarily give the CPU to the next runnable task.
pub fn yield_now() {
    let (old, new_rsp) = {
        let mut manager = TASKS.lock();
        if !manager.started {
            return;
        }

        let old = manager.current;
        let mut next = old;
        for step in 1..=MAX_TASKS {
            let candidate = (old + step) % MAX_TASKS;
            if manager.tasks[candidate].state == TaskState::Ready {
                next = candidate;
                break;
            }
        }
        if next == old {
            return;
        }
        manager.current = next;
        (old, unsafe { RSPS[next] })
        // The guard is dropped here. Switching while holding it would
        // deadlock the incoming task when it locks TASKS.
    };

    unsafe { switch_context(&mut RSPS[old], new_rsp) }
}

extern "C" fn task_trampoline() {
    let func = {
        let manager = TASKS.lock();
        manager.tasks[manager.current].func
    };

    func();

    {
        let mut manager = TASKS.lock();
        let current = manager.current;
        manager.tasks[current].state = TaskState::Terminated;
    }

    loop {
        yield_now();
    }
}

/// Run a command entered at the shell prompt.
fn execute_command(line: &str) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    let (command, args) = match line.find(' ') {
        Some(index) => (&line[..index], Some(&line[index + 1..])),
        None => (line, None),
    };

    match command {
        "help" => {
            diagnostics::println("Available commands:");
            diagnostics::println("  help      show this list");
            diagnostics::println("  version   show OS version");
            diagnostics::println("  about     about Zenith OS");
            diagnostics::println("  echo      print text");
            diagnostics::println("  uptime    seconds since boot");
            diagnostics::println("  clear     clear the screen");
            diagnostics::println("  flip      change display orientation");
        }
        "version" => diagnostics::println("Zenith OS v0.1.0 (x86_64, Limine)"),
        "about" => {
            diagnostics::println("Zenith OS - an independent x86_64 operating");
            diagnostics::println("system written in Rust and assembly.");
        }
        "echo" => {
            if let Some(text) = args {
                diagnostics::println(text);
            }
        }
        "uptime" => {
            diagnostics::puts("uptime: ");
            diagnostics::put_dec(timer::ticks() / 100);
            diagnostics::println(" s");
        }
        "clear" => diagnostics::clear_screen(),
        "flip" => crate::framebuffer::cycle_orientation(),
        other => {
            diagnostics::puts("unknown command: ");
            diagnostics::println(other);
            diagnostics::println("Type 'help' for a command list.");
        }
    }
}

/// Interactive line-editing shell over the PS/2 keyboard.
fn shell_task() {
    diagnostics::println("Zenith OS v0.1.0 - interactive shell");
    diagnostics::println("Type 'help' for available commands.");
    diagnostics::puts("zenith> ");

    let mut line = [0u8; 128];
    let mut length = 0usize;

    loop {
        // F12 cycles the framebuffer orientation for VMs with rotated displays.
        if let Some(scancode) = keyboard::take_raw() {
            if scancode == 0x58 {
                crate::framebuffer::cycle_orientation();
            }
        }

        if let Some(byte) = keyboard::read_char() {
            match byte {
                b'\r' | b'\n' => {
                    diagnostics::putc(b'\n');
                    if length > 0 {
                        let command = core::str::from_utf8(&line[..length]).unwrap_or("");
                        execute_command(command);
                        length = 0;
                    }
                    diagnostics::puts("zenith> ");
                }
                0x08 => {
                    if length > 0 {
                        length -= 1;
                        diagnostics::puts("\x08 \x08");
                    }
                }
                other if length + 1 < line.len() => {
                    line[length] = other;
                    length += 1;
                    diagnostics::putc(other);
                }
                _ => {}
            }
        }
        yield_now();
    }
}

/// Keeps the scheduler and NIC alive; tick logs go to COM1 only.
fn heartbeat_task() {
    let mut next = timer::ticks() + 100;
    loop {
        if timer::ticks() >= next {
            next = timer::ticks() + 100;
            crate::e1000::poll();
            diagnostics::serial_puts("[sched] tick=");
            diagnostics::serial_put_hex(timer::ticks());
            diagnostics::serial_puts("\n");
        }
        yield_now();
    }
}

fn idle_task() {
    loop {
        yield_now();
    }
}
