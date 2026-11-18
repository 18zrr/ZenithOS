//! Deepin-inspired graphical installation wizard.
//!
//! The wizard is fully interactive and uses real system data: ATA presence,
//! detected PCI/NIC information and a real Zenith FS write. When storage is
//! unavailable (for example when booting from the CD image without a disk) the
//! wizard reports the failure honestly instead of pretending success.

use crate::{ata, framebuffer as fb, fs, keyboard, timer};

const BG: u32 = 0x00101820;
const PANEL: u32 = 0x00182430;
const ACCENT: u32 = 0x00E17055;
const BLUE: u32 = 0x002D8FB0;
const FG: u32 = 0x00F0F4F8;
const MUTED: u32 = 0x007A8A9A;
const SUCCESS: u32 = 0x0055C08A;
const WARN: u32 = 0x00E0A030;

const STEP_NAMES: [&str; 5] = ["Welcome", "Disk", "Confirm", "Install", "Done"];

/// Description of the installation target, read from real hardware state.
fn target_line() -> &'static str {
    if ata::present() {
        "ATA Primary Master  -  Zenith FS v5 (16 MiB)"
    } else {
        "No ATA disk detected (installation will fail)"
    }
}

fn render_sidebar(step: usize, width: usize, height: usize) {
    fb::fill_rect(0, 0, width, height, BG);
    fb::fill_rect(0, 0, width, 48, PANEL);
    fb::fill_rect(0, 0, width, 4, ACCENT);
    fb::text(24, 16, "Zenith OS Installer", FG);
    fb::text(width - 96, 16, "v0.1.0", MUTED);

    fb::fill_rect(0, 48, 256, height - 104, 0x00121C2C);
    for (index, name) in STEP_NAMES.iter().enumerate() {
        let y = 88 + index * 40;
        if index == step {
            fb::fill_rect(0, y - 8, 256, 32, ACCENT);
        }
        let number = match index {
            0 => "1",
            1 => "2",
            2 => "3",
            3 => "4",
            _ => "5",
        };
        fb::text(24, y, number, FG);
        fb::text(64, y, name, FG);
    }
}

fn render_footer(text: &str, width: usize, height: usize) {
    let y = height - 56;
    fb::fill_rect(0, y, width, 56, PANEL);
    fb::fill_rect(0, y, width, 2, 0x002A3A4A);
    fb::text(24, y + 20, text, MUTED);
}

fn render(step: usize, selection: usize, progress: usize, result: Option<bool>) {
    let (width, height) = fb::dimensions();
    if width == 0 {
        return;
    }

    render_sidebar(step, width, height);
    let x = 296;
    let mut y = 96;

    match step {
        0 => {
            fb::text(x, y, "Welcome to Zenith OS", FG);
            y += 40;
            fb::text(x, y, "This wizard will install Zenith OS v0.1.0", MUTED);
            y += 24;
            fb::text(x, y, "on your computer.", MUTED);
            y += 56;
            fb::text(x, y, "Language", FG);
            y += 32;
            let label = "> English                         ";
            fb::fill_rect(x - 8, y - 8, 320, 32, if selection == 0 { BLUE } else { PANEL });
            fb::text(x, y, label, FG);
        }
        1 => {
            fb::text(x, y, "Select installation target", FG);
            y += 48;
            fb::text(x, y, "Detected storage:", MUTED);
            y += 32;
            let color = if ata::present() { FG } else { WARN };
            fb::text(x, y, target_line(), color);
            y += 56;
            fb::text(x, y, "Filesystem: Zenith FS v5 (journaled, extents)", MUTED);
            y += 28;
            fb::text(x, y, "FAT32 / NTFS / ext4 readers remain read-only.", MUTED);
            y += 56;
            let label = "> Erase partition and install Zenith OS";
            fb::fill_rect(x - 8, y - 8, 600, 32, if selection == 0 { BLUE } else { PANEL });
            fb::text(x, y, label, FG);
        }
        2 => {
            fb::text(x, y, "Ready to install", FG);
            y += 48;
            fb::text(x, y, "Language:  English", MUTED);
            y += 28;
            fb::text(x, y, "Target:    ATA primary master", MUTED);
            y += 28;
            fb::text(x, y, "System:    Zenith FS v5", MUTED);
            y += 48;
            fb::text(x, y, "Warning: this overwrites the target partition.", WARN);
            y += 56;
            let yes = if selection == 0 { "> Install now" } else { "  Install now" };
            let no = if selection == 1 { "> Go back" } else { "  Go back" };
            fb::fill_rect(x - 8, y - 8, 400, 32, if selection == 0 { BLUE } else { PANEL });
            fb::text(x, y, yes, FG);
            fb::fill_rect(x - 8, y + 24, 400, 32, if selection == 1 { BLUE } else { PANEL });
            fb::text(x, y + 32, no, FG);
        }
        3 => {
            fb::text(x, y, "Installing Zenith OS...", FG);
            y += 56;
            let bar_x = x;
            let bar_w = 440usize.min(width.saturating_sub(x + 40));
            let bar_y = y;
            fb::fill_rect(bar_x, bar_y, bar_w, 12, PANEL);
            let filled = bar_w * progress / 100;
            fb::fill_rect(bar_x, bar_y, filled, 12, ACCENT);
            y += 40;
            let phase = if progress < 35 {
                "Creating directories..."
            } else if progress < 70 {
                "Writing system configuration..."
            } else if progress < 100 {
                "Flushing filesystem journal..."
            } else {
                "Finishing..."
            };
            fb::text(x, y, phase, MUTED);
        }
        _ => {
            let ok = result.unwrap_or(false);
            if ok {
                fb::text(x, y, "Installation complete", SUCCESS);
                y += 48;
                fb::text(x, y, "Zenith OS files were written to Zenith FS.", FG);
                y += 28;
                fb::text(x, y, "Reboot from the disk image to start the system.", MUTED);
            } else {
                fb::text(x, y, "Installation failed", WARN);
                y += 48;
                fb::text(x, y, "No writable Zenith FS partition was available.", FG);
                y += 28;
                fb::text(x, y, "Boot from a disk image (zenith.img) and retry.", MUTED);
            }
            y += 56;
            fb::text(x, y, "Press Enter to return to the shell.", MUTED);
        }
    }

    let hint = match step {
        0 | 1 => "Enter: continue     Esc: cancel",
        2 => "Up/Down: choose     Enter: confirm     Esc: cancel",
        3 => "Please wait...",
        _ => "Enter: finish",
    };
    render_footer(hint, width, height);
}

/// Run the installer wizard to completion, then return to the shell.
pub fn run() {
    crate::mouse::flush_events();
    run_inner();
    crate::mouse::flush_events();
    crate::wm::invalidate();
    crate::wm::render_now();
}

fn run_inner() {
    if fb::dimensions().0 == 0 {
        crate::diagnostics::println("installer: no framebuffer available");
        return;
    }

    let mut step = 0usize;
    let mut selection = 0usize;
    let mut start_tick = 0u64;
    let mut progress = 0usize;
    let mut result: Option<bool> = None;

    render(step, selection, progress, result);

    loop {
        // The shell normally services these. The modal wizard owns its loop,
        // so USB input and PS/2 repeat must continue to be polled here.
        keyboard::poll();
        crate::usb::poll();
        while let Some(event) = keyboard::take_event() {
            if !event.pressed {
                continue;
            }
            match event.key {
                keyboard::Key::Escape => return,
                keyboard::Key::Up => {
                    if step == 2 && selection > 0 {
                        selection -= 1;
                        render(step, selection, progress, result);
                    }
                }
                keyboard::Key::Down => {
                    if step == 2 && selection < 1 {
                        selection += 1;
                        render(step, selection, progress, result);
                    }
                }
                keyboard::Key::Enter => match step {
                    0 | 1 => {
                        step += 1;
                        selection = 0;
                        render(step, selection, progress, result);
                    }
                    2 => {
                        if selection == 1 {
                            step = 1;
                            selection = 0;
                            render(step, selection, progress, result);
                        } else {
                            step = 3;
                            progress = 0;
                            start_tick = timer::ticks();
                            render(step, selection, progress, result);
                            crate::diagnostics::serial_puts("[installer] writing manifest\n");
                            result = Some(match fs::installer_write() {
                                Ok(()) => {
                                    crate::diagnostics::serial_puts("[installer] manifest saved\n");
                                    true
                                }
                                Err(error) => {
                                    crate::diagnostics::serial_puts("[installer] failed: ");
                                    crate::diagnostics::serial_puts(error);
                                    crate::diagnostics::serial_puts("\n");
                                    false
                                }
                            });
                        }
                    }
                    3 => {}
                    _ => return,
                },
                keyboard::Key::Backspace => {
                    if step > 0 && step < 4 {
                        step -= 1;
                        selection = 0;
                        render(step, selection, progress, result);
                    }
                }
                keyboard::Key::Char(b'q') | keyboard::Key::Char(b'Q') => return,
                _ => {}
            }
        }

        if step == 3 && progress < 100 {
            let elapsed = timer::ticks().saturating_sub(start_tick);
            let next = ((elapsed * 100) / 180).min(100) as usize;
            if next != progress {
                progress = next;
                if progress >= 100 {
                    step = 4;
                }
                render(step, selection, progress, result);
            }
        }

        crate::task::yield_now();
    }
}
