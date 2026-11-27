//! Page navigation and live information for the compositor's Settings window.
use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Page {
    System,
    Display,
    Appearance,
    Storage,
    Devices,
    Input,
    Network,
    About,
}

const PAGES: [(Page, &str); 8] = [
    (Page::System, "System"),
    (Page::Display, "Display"),
    (Page::Appearance, "Style"),
    (Page::Storage, "Storage"),
    (Page::Devices, "Devices"),
    (Page::Input, "Input"),
    (Page::Network, "Network"),
    (Page::About, "About"),
];

impl Page {
    pub(super) fn adjacent(self, back: bool) -> Self {
        let i = PAGES
            .iter()
            .position(|(page, _)| *page == self)
            .unwrap_or(0);
        let step = if back { PAGES.len() - 1 } else { 1 };
        PAGES[(i + step) % PAGES.len()].0
    }
}

fn tab(window: &Window, i: usize) -> Rect {
    let y = window.y + TITLEBAR_H as i32 + 14;
    if window.w >= 560 {
        Rect::new(window.x + 10, y + i as i32 * 34, 112, 28)
    } else {
        let width = (window.w - 20) / 4;
        Rect::new(
            window.x + 10 + (i % 4 * width) as i32,
            y + (i / 4 * 34) as i32,
            width - 4,
            28,
        )
    }
}

pub(super) fn content(window: &Window) -> Window {
    if window.w >= 560 {
        Window {
            x: window.x + 128,
            w: window.w - 128,
            ..*window
        }
    } else {
        Window {
            y: window.y + 72,
            ..*window
        }
    }
}

pub(super) fn page_at(window: &Window, x: i32, y: i32) -> Option<Page> {
    PAGES
        .iter()
        .enumerate()
        .find(|(i, _)| tab(window, *i).contains(x, y))
        .map(|(_, p)| p.0)
}

pub(super) fn interval_button(window: &Window) -> Rect {
    Rect::new(window.x + 30, window.y + TITLEBAR_H as i32 + 172, 230, 28)
}

impl Wm {
    pub(super) fn cycle_interval(&mut self) {
        self.prefs.sample_seconds = match self.prefs.sample_seconds {
            1 => 2,
            2 => 5,
            _ => 1,
        };
        self.save_preferences();
        invalidate();
    }
    pub(super) fn draw_settings(&self, f: &mut [u32], window: &Window, mx: i32, my: i32) {
        let height = f.len() / self.width;
        for (i, (page, name)) in PAGES.iter().enumerate() {
            let r = tab(window, i);
            draw_button(
                f,
                self.width,
                height,
                &r,
                name,
                *page == self.settings_page,
                r.contains(mx, my),
                self.accent,
            );
        }
        let w = content(window);
        let x = w.x + 16;
        let width = w.w - 32;
        let y = w.y + TITLEBAR_H as i32 + 14;
        let left = x + 14;
        let right = x + width as i32 - 14;
        match self.settings_page {
            Page::Appearance => self.draw_appearance(f, &w, mx, my),
            Page::Display => self.draw_display(f, &w, mx, my),
            Page::Input => self.draw_input(f, &w),
            Page::About => {
                self.card(f, x, y, width, 150, "About ZenithOS");
                draw_text(
                    f,
                    self.width,
                    height,
                    left,
                    y + 42,
                    right,
                    env!("ZENITH_RELEASE_ASCII"),
                    FG,
                );
                self.monitor_row(f, left, y + 72, right, "Kernel", env!("CARGO_PKG_VERSION"));
                self.monitor_row(f, left, y + 102, right, "License", "MIT");
            }
            Page::System => {
                self.card(f, x, y, width, 244, "System");
                self.monitor_row(f, left, y + 42, right, "Architecture", "x86_64");
                let mut mem = TextBuffer::new();
                mem.push_dec(limine::memory_summary().1 >> 20);
                mem.push_str(" MiB usable at boot");
                self.monitor_row(f, left, y + 68, right, "Memory map", mem.as_str());
                let mut uptime = TextBuffer::new();
                uptime.push_dec(timer::ticks() / 100);
                uptime.push_str(" seconds");
                self.monitor_row(f, left, y + 94, right, "Uptime", uptime.as_str());
                self.monitor_row(
                    f,
                    left,
                    y + 120,
                    right,
                    "Boot",
                    if limine::is_active() { "Limine" } else { "PVH" },
                );
                let r = interval_button(&w);
                let mut cadence = TextBuffer::new();
                cadence.push_str("Monitor interval: ");
                cadence.push_dec(self.prefs.sample_seconds as u64);
                cadence.push_str(" s");
                draw_button(
                    f,
                    self.width,
                    height,
                    &r,
                    cadence.as_str(),
                    false,
                    r.contains(mx, my),
                    self.accent,
                );
                let cpu = core::arch::x86_64::__cpuid(0);
                let mut vendor = [0; 12];
                vendor[..4].copy_from_slice(&cpu.ebx.to_le_bytes());
                vendor[4..8].copy_from_slice(&cpu.edx.to_le_bytes());
                vendor[8..].copy_from_slice(&cpu.ecx.to_le_bytes());
                self.monitor_row(
                    f,
                    left,
                    y + 202,
                    right,
                    "CPU vendor",
                    core::str::from_utf8(&vendor).unwrap_or("Unknown"),
                );
            }
            Page::Storage => {
                self.card(f, x, y, width, 162, "Storage");
                self.monitor_row(
                    f,
                    left,
                    y + 42,
                    right,
                    "ATA primary",
                    if crate::ata::present() {
                        "Detected"
                    } else {
                        "Not detected"
                    },
                );
                let storage = crate::fs::volume(|fs| {
                    let sb = fs.superblock();
                    Ok((
                        sb.total_blocks * sb.block_size,
                        sb.free_blocks * sb.block_size,
                    ))
                });
                if let Ok((total, free)) = storage {
                    for (i, (label, bytes)) in
                        [("Volume", total), ("Used", total - free), ("Free", free)]
                            .iter()
                            .enumerate()
                    {
                        let mut value = TextBuffer::new();
                        value.push_dec((*bytes / 1024) as u64);
                        value.push_str(" KiB");
                        self.monitor_row(
                            f,
                            left,
                            y + 68 + i as i32 * 26,
                            right,
                            label,
                            value.as_str(),
                        );
                    }
                } else {
                    self.monitor_row(f, left, y + 68, right, "Zenith FS", "Not mounted");
                }
            }
            Page::Devices => {
                self.card(f, x, y, width, 150, "Devices");
                self.monitor_row(f, left, y + 42, right, "Display", display::backend_name());
                self.monitor_row(f, left, y + 68, right, "Pointer", mouse::source());
                self.monitor_row(f, left, y + 94, right, "GPU counters", "Unavailable");
            }
            Page::Network => {
                self.card(f, x, y, width, 166, "Network");
                let mac = e1000::mac();
                if mac == [0; 6] {
                    self.monitor_row(f, left, y + 42, right, "Interface", "No e1000 detected");
                } else {
                    self.monitor_row(
                        f,
                        left,
                        y + 42,
                        right,
                        "e1000 link",
                        if e1000::link_up() { "Up" } else { "Down" },
                    );
                    let mut ip = TextBuffer::new();
                    for (i, octet) in crate::net::our_ip().iter().enumerate() {
                        if i != 0 {
                            ip.push(b'.');
                        }
                        ip.push_dec(u64::from(*octet));
                    }
                    self.monitor_row(f, left, y + 68, right, "IPv4", ip.as_str());
                    let mut addr = TextBuffer::new();
                    for (i, byte) in mac.iter().enumerate() {
                        if i != 0 {
                            addr.push(b':');
                        }
                        let hex = b"0123456789abcdef";
                        addr.push(hex[(byte >> 4) as usize]);
                        addr.push(hex[(byte & 15) as usize]);
                    }
                    self.monitor_row(f, left, y + 94, right, "MAC", addr.as_str());
                }
            }
        }
        if matches!(self.settings_page, Page::System | Page::Appearance) {
            let status =
                core::str::from_utf8(&self.display_status[..self.display_status_len]).unwrap_or("");
            draw_text(f, self.width, height, left, y + 250, right, status, MUTED);
        }
    }
}
