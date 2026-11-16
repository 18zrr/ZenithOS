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
        let frame = top - 56; // r15, r14, r13, r12, rbx, rbp, return address
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

/// A tiny interactive shell over the PS/2 keyboard and VGA console.
fn shell_task() {
    diagnostics::puts("\n[shell] ready - type to echo, backspace to delete\n");
    let mut last_ticks = timer::ticks();
    loop {
        if let Some(byte) = keyboard::read_char() {
            match byte {
                b'\r' | b'\n' => {
                    diagnostics::putc(b'\n');
                    diagnostics::puts("zenith> ");
                }
                0x08 => diagnostics::puts("\x08 \x08"),
                other => diagnostics::putc(other),
            }
        }
        if timer::ticks() != last_ticks {
            last_ticks = timer::ticks();
        }
        yield_now();
    }
}

/// Demonstrates preemption-friendly cooperative scheduling with a tick counter.
fn heartbeat_task() {
    diagnostics::puts("[sched] scheduler online\n");
    let mut next = timer::ticks() + 100;
    loop {
        if timer::ticks() >= next {
            next = timer::ticks() + 100;
            crate::e1000::poll();
            diagnostics::puts("[sched] tick=");
            diagnostics::put_hex(timer::ticks());
            diagnostics::puts("\n");
        }
        yield_now();
    }
}

fn idle_task() {
    loop {
        yield_now();
    }
}
