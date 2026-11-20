//! Cooperative task scheduler with per-task kernel stacks.

use crate::diagnostics;
use crate::keyboard;
use crate::sync::SpinLock;
use crate::timer;

const MAX_TASKS: usize = 3;
// Filesystem mount alone uses over 17 KiB in the release build, on top of
// the shell history, installer and interrupt frames. Keep room for that
// nested call chain; 16 KiB corrupted neighbouring scheduler state.
const STACK_SIZE: usize = 128 * 1024;

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

fn put_hex_byte(byte: u8) {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    diagnostics::putc(DIGITS[(byte >> 4) as usize]);
    diagnostics::putc(DIGITS[(byte & 0x0F) as usize]);
}

fn print_mac(mac: [u8; 6]) {
    for index in 0..6 {
        if index > 0 {
            diagnostics::puts(":");
        }
        put_hex_byte(mac[index]);
    }
}

fn print_ip(ip: [u8; 4]) {
    for index in 0..4 {
        if index > 0 {
            diagnostics::puts(".");
        }
        diagnostics::put_dec(ip[index] as u64);
    }
}

fn redraw_line(prompt: &str, text: &str) {
    diagnostics::puts("\r");
    for _ in 0..(prompt.len() + text.len() + 2) {
        diagnostics::puts(" ");
    }
    diagnostics::puts("\r");
    diagnostics::puts(prompt);
    diagnostics::puts(text);
}

/// Run a command entered at the shell prompt.
fn execute_command(line: &str, history: &[[u8; 128]], history_len: usize) {
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
            diagnostics::println("  browser   open the Chromium companion");
            diagnostics::println("  sysinfo   system overview");
            diagnostics::println("  pci       list PCI devices");
            diagnostics::println("  mem       memory summary");
            diagnostics::println("  net       network status");
            diagnostics::println("  gpu       detected GPU");
            diagnostics::println("  acpi      ACPI table pointer");
            diagnostics::println("  history   recent commands");
            diagnostics::println("  version   show OS version");
            diagnostics::println("  about     about Zenith OS");
            diagnostics::println("  echo      print text");
            diagnostics::println("  uptime    seconds since boot");
            diagnostics::println("  clear     clear the screen");
            diagnostics::println("  flip      change display orientation");
            diagnostics::println("  display   show or set WIDTHxHEIGHT");
            diagnostics::println("  pointer   mouse cursor position");
            diagnostics::println("  install   graphical installation wizard");
            diagnostics::println("F1 opens the Start menu; F12 changes display orientation.");
        }
        "browser" => crate::wm::open_browser(),
        "sysinfo" => {
            diagnostics::println("Zenith OS v0.3.0 - system overview");
            diagnostics::puts("boot: ");
            diagnostics::println(if crate::limine::is_active() { "Limine native" } else { "PVH direct" });
            let (width, height) = crate::framebuffer::dimensions();
            if width != 0 && height != 0 {
                diagnostics::puts("display: ");
                diagnostics::put_dec(width as u64);
                diagnostics::puts("x");
                diagnostics::put_dec(height as u64);
                diagnostics::puts("x32  ");
                diagnostics::puts(crate::display::backend_name());
                diagnostics::puts("\n");
            }
            let (regions, usable) = crate::limine::memory_summary();
            diagnostics::puts("memory: ");
            diagnostics::put_dec(usable / (1024 * 1024));
            diagnostics::puts(" MiB usable in ");
            diagnostics::put_dec(regions);
            diagnostics::println(" regions");
            diagnostics::puts("hddm: ");
            diagnostics::put_hex(crate::limine::hhdm_offset());
            diagnostics::puts("\n");
            diagnostics::puts("mac: ");
            print_mac(crate::e1000::mac());
            diagnostics::puts("  link: ");
            diagnostics::println(if crate::e1000::link_up() { "up" } else { "down" });
        }
        "pci" => {
            let mut count = 0u64;
            for bus in 0..8u8 {
                for slot in 0..32u8 {
                    let ids = crate::pci::read(bus, slot, 0, 0);
                    let vendor = ids & 0xFFFF;
                    if vendor == 0xFFFF {
                        continue;
                    }
                    let class = crate::pci::read(bus, slot, 0, 8) >> 24;
                    diagnostics::puts("  ");
                    diagnostics::put_hex(bus as u64);
                    diagnostics::puts(":");
                    diagnostics::put_hex(slot as u64);
                    diagnostics::puts(" vendor=");
                    diagnostics::put_hex(vendor as u64);
                    diagnostics::puts(" device=");
                    diagnostics::put_hex((ids >> 16) as u64);
                    diagnostics::puts(" class=");
                    diagnostics::put_hex(class as u64);
                    diagnostics::puts("\n");
                    count += 1;
                }
            }
            diagnostics::puts("pci devices: ");
            diagnostics::put_dec(count);
            diagnostics::puts("\n");
        }
        "mem" => {
            let (regions, usable) = crate::limine::memory_summary();
            diagnostics::puts("usable memory: ");
            diagnostics::put_dec(usable / (1024 * 1024));
            diagnostics::puts(" MiB (");
            diagnostics::put_dec(usable);
            diagnostics::puts(" bytes) in ");
            diagnostics::put_dec(regions);
            diagnostics::println(" regions");
        }
        "net" => {
            diagnostics::puts("mac: ");
            print_mac(crate::e1000::mac());
            diagnostics::puts("\nlink: ");
            diagnostics::println(if crate::e1000::link_up() { "up" } else { "down" });
            diagnostics::puts("ip: ");
            print_ip(crate::net::our_ip());
            diagnostics::puts("\ngateway: ");
            print_ip(crate::net::gateway());
            diagnostics::puts("\n");
            diagnostics::println("note: transmit works; RX is not validated");
        }
        "gpu" => match crate::nvidia::detected() {
            Some(info) => {
                diagnostics::puts("gpu: ");
                diagnostics::println(crate::nvidia::region_name(info.device));
                diagnostics::puts("  device=");
                diagnostics::put_hex(info.device as u64);
                diagnostics::puts(" boot0=");
                diagnostics::put_hex(info.boot0 as u64);
                diagnostics::puts("\n  bar0=");
                diagnostics::put_hex(info.bar0);
                diagnostics::puts("\n");
                diagnostics::println("  detection/probing only - no driver");
            }
            None => diagnostics::println("gpu: no NVIDIA display controller detected"),
        },
        "acpi" => {
            let rsdp = {
                let limine = crate::limine::rsdp_address();
                if limine != 0 { limine } else { crate::framebuffer::mb2_rsdp() }
            };
            if rsdp != 0 {
                diagnostics::puts("acpi rsdp=");
                diagnostics::put_hex(rsdp);
                diagnostics::puts("\n");
            } else {
                diagnostics::println("acpi: no RSDP reported");
            }
        }
        "history" => {
            if history_len == 0 {
                diagnostics::println("history: empty");
            } else {
                for (index, entry) in history.iter().take(history_len).enumerate() {
                    let length = entry.iter().position(|&byte| byte == 0).unwrap_or(entry.len());
                    diagnostics::put_dec(index as u64 + 1);
                    diagnostics::puts("  ");
                    diagnostics::println(core::str::from_utf8(&entry[..length]).unwrap_or(""));
                }
            }
        }
        "version" => diagnostics::println("Zenith OS v0.3.0 (x86_64, Limine)"),
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
        "display" => match args {
            Some(text) => match crate::display::parse(text) {
                Some((width, height)) => match crate::wm::apply_display(width, height) {
                    Ok(()) => {
                        diagnostics::puts("[display] applied ");
                        diagnostics::put_dec(width as u64);
                        diagnostics::puts("x");
                        diagnostics::put_dec(height as u64);
                        diagnostics::puts("\n");
                    }
                    Err(message) => {
                        diagnostics::puts("[display] ");
                        diagnostics::println(message);
                    }
                },
                None => diagnostics::println("usage: display WIDTHxHEIGHT (for example 1920x1080)"),
            },
            None => {
                let (width, height) = crate::framebuffer::dimensions();
                diagnostics::puts("display: ");
                diagnostics::put_dec(width as u64);
                diagnostics::puts("x");
                diagnostics::put_dec(height as u64);
                diagnostics::puts(" backend=");
                diagnostics::puts(crate::display::backend_name());
                diagnostics::puts("\n");
                diagnostics::println("refresh: controlled by the host display, not the guest");
                diagnostics::println("physical GPUs need a native display driver (not implemented)");
            }
        },
        "pointer" => {
            let (x, y) = crate::mouse::position();
            diagnostics::puts("pointer: ");
            diagnostics::put_dec(x.max(0) as u64);
            diagnostics::puts(",");
            diagnostics::put_dec(y.max(0) as u64);
            diagnostics::puts("\n");
        }
        "install" => {
            crate::mouse::flush_events();
            crate::installer::run();
            crate::mouse::flush_events();
            crate::wm::invalidate();
        }
        other => {
            diagnostics::puts("unknown command: ");
            diagnostics::println(other);
            diagnostics::println("Type 'help' for a command list.");
        }
    }
}

/// Interactive line-editing shell over the PS/2 keyboard.
fn shell_task() {
    const CAPACITY: usize = 16;

    diagnostics::println("Zenith OS v0.3.0 - interactive shell");
    diagnostics::println("Type 'help' for available commands.");
    diagnostics::puts("zenith> ");

    let mut line = [0u8; 128];
    let mut length = 0usize;
    let mut history = [[0u8; 128]; CAPACITY];
    let mut history_len = 0usize;
    let mut history_index = 0usize;

    loop {
        keyboard::poll();
        crate::usb::poll();

        // Mouse events drive the window manager.
        match crate::wm::pump() {
            crate::wm::Action::Installer => {
                run_installer();
                continue;
            }
            crate::wm::Action::DisplayMode(width, height) => {
                let _ = crate::wm::apply_display(width, height);
                continue;
            }
            crate::wm::Action::None => {}
        }

        while let Some(event) = keyboard::take_event() {
            if !event.pressed {
                continue;
            }
            match event.key {
                keyboard::Key::F(1) => {
                    crate::wm::toggle_menu();
                    continue;
                }
                keyboard::Key::F(12) => {
                    crate::framebuffer::cycle_orientation();
                    continue;
                }
                _ => {}
            }

            // While the Start menu is open it owns all keyboard input.
            if crate::start_menu::is_open() {
                if crate::wm::menu_key(event) == crate::wm::Action::Installer {
                    run_installer();
                }
                continue;
            }

            if crate::wm::browser_key(event) {
                continue;
            }
            if crate::wm::scroll_key(event.key) {
                continue;
            }

            // The Settings custom-resolution field owns typing while active.
            if crate::wm::settings_input_active() {
                if let Some(action) = crate::wm::settings_key(event) {
                    if let crate::wm::Action::DisplayMode(width, height) = action {
                        let _ = crate::wm::apply_display(width, height);
                    }
                }
                continue;
            }

            // Typing anywhere on the desktop brings the terminal forward,
            // instead of silently disappearing when Settings or another
            // window has focus.
            let edits_terminal = matches!(
                event.key,
                keyboard::Key::Char(_)
                    | keyboard::Key::Backspace
                    | keyboard::Key::Enter
                    | keyboard::Key::Up
                    | keyboard::Key::Down
                    | keyboard::Key::PageUp
                    | keyboard::Key::PageDown
            );
            if edits_terminal && !crate::wm::terminal_focused() {
                crate::wm::focus_terminal();
            }

            // Typing belongs to the terminal only while it has focus.
            if !crate::wm::terminal_focused() {
                continue;
            }

            match event.key {
                keyboard::Key::Up => {
                    if history_index > 0 {
                        history_index -= 1;
                        length = 0;
                        while length < line.len() && history[history_index][length] != 0 {
                            length += 1;
                        }
                        line[..length].copy_from_slice(&history[history_index][..length]);
                        redraw_line("zenith> ", core::str::from_utf8(&line[..length]).unwrap_or(""));
                    }
                }
                keyboard::Key::Down => {
                    if history_index + 1 < history_len {
                        history_index += 1;
                        length = 0;
                        while length < line.len() && history[history_index][length] != 0 {
                            length += 1;
                        }
                        line[..length].copy_from_slice(&history[history_index][..length]);
                        redraw_line("zenith> ", core::str::from_utf8(&line[..length]).unwrap_or(""));
                    } else if history_index < history_len {
                        history_index = history_len;
                        length = 0;
                        redraw_line("zenith> ", "");
                    }
                }
                keyboard::Key::PageUp => crate::framebuffer::scroll_lines(10),
                keyboard::Key::PageDown => crate::framebuffer::scroll_lines(-10),
                keyboard::Key::Enter => {
                    diagnostics::puts("\n");
                    if length > 0 {
                        let command = core::str::from_utf8(&line[..length]).unwrap_or("");
                        let slot = if history_len < CAPACITY {
                            let slot = history_len;
                            history_len += 1;
                            slot
                        } else {
                            for index in 0..CAPACITY - 1 {
                                history[index] = history[index + 1];
                            }
                            CAPACITY - 1
                        };
                        history[slot] = [0u8; 128];
                        history[slot][..length].copy_from_slice(&line[..length]);
                        history_index = history_len;

                        execute_command(command, &history, history_len);
                        length = 0;
                    }
                    diagnostics::puts("zenith> ");
                }
                keyboard::Key::Backspace => {
                    if length > 0 {
                        length -= 1;
                        diagnostics::puts("\x08 \x08");
                    }
                }
                keyboard::Key::Char(b'l') if event.ctrl => crate::framebuffer::clear_screen(),
                keyboard::Key::Char(b'c') if event.ctrl => {
                    length = 0;
                    diagnostics::println("^C");
                    diagnostics::puts("zenith> ");
                }
                keyboard::Key::Char(other) if (0x20..=0x7E).contains(&other) => {
                    if length + 1 < line.len() {
                        line[length] = other;
                        length += 1;
                        diagnostics::console_putc(other);
                    }
                }
                _ => {}
            }
        }

        crate::wm::tick();
        yield_now();
    }
}

/// Run the modal installer without letting stale mouse events leak into the
/// desktop when it returns.
fn run_installer() {
    crate::mouse::flush_events();
    crate::installer::run();
    crate::mouse::flush_events();
    crate::wm::invalidate();
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
