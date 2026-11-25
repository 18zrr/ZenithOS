//! File Manager: cache the directory listing between input events, never do I/O
//! while painting a frame.
use super::*;
use crate::fs::volume;
use alloc::{
    format,
    string::{String, ToString},
};
use zenith_fs::FsError;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Browse,
    File,
    Folder,
    Rename,
    Copy,
    Delete,
}

pub(super) struct Files {
    dir: String,
    entries: Vec<(String, bool)>,
    selected: usize,
    mode: Mode,
    input: String,
    status: String,
}

const TOOLS: [&str; 7] = [
    "Up", "Open", "+ File", "+ Folder", "Rename", "Copy", "Delete",
];

impl Files {
    pub const fn new() -> Self {
        Self {
            dir: String::new(),
            entries: Vec::new(),
            selected: 0,
            mode: Mode::Browse,
            input: String::new(),
            status: String::new(),
        }
    }

    pub fn refresh(&mut self) {
        self.mode = Mode::Browse;
        self.entries.clear();
        match volume(|fs| fs.list(&self.dir)) {
            Ok(mut entries) => {
                entries.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
                self.entries = entries;
                self.status = format!("{} entries", self.entries.len());
            }
            Err(FsError::InvalidMagicNumber) => self.status = "No mounted Zenith FS disk".into(),
            Err(e) => self.status = e.to_string(),
        }
        self.selected = self.selected.min(self.entries.len().saturating_sub(1));
    }

    fn tool(&mut self, index: usize) {
        self.mode = Mode::Browse;
        self.input.clear();
        if index == 0 {
            if let Some(i) = self.dir.rfind('/') {
                self.dir.truncate(i);
            }
            self.selected = 0;
            self.refresh();
            return;
        }
        if index == 2 || index == 3 {
            self.mode = if index == 2 { Mode::File } else { Mode::Folder };
            return;
        }
        let Some((name, folder)) = self.entries.get(self.selected) else {
            return;
        };
        let path = format!("{}/{}", self.dir, name);
        match index {
            1 if *folder => {
                self.dir = path;
                self.selected = 0;
                self.refresh();
            }
            1 => {
                self.status = match volume(|fs| {
                    let id = fs.resolve(&path)?;
                    Ok((fs.stat(id)?.0, fs.read_range(id, 0, 96)?))
                }) {
                    Ok((size, data)) => format!(
                        "{} bytes  |  {}",
                        size,
                        core::str::from_utf8(&data)
                            .ok()
                            .and_then(|s| s.lines().next())
                            .filter(|s| !s.chars().any(char::is_control))
                            .unwrap_or("(no text preview)")
                    ),
                    Err(e) => e.to_string(),
                };
            }
            4 => {
                self.mode = Mode::Rename;
            }
            5 if !folder => {
                self.mode = Mode::Copy;
            }
            5 => self.status = "Select a file to copy".into(),
            6 => self.mode = Mode::Delete,
            _ => {}
        }
    }

    fn submit(&mut self) {
        let mode = self.mode;
        if mode != Mode::Delete
            && (self.input.is_empty()
                || self.input == "."
                || self.input == ".."
                || self.input.contains('/'))
        {
            self.mode = Mode::Browse;
            self.status = "Use a single non-empty name".into();
            return;
        }
        let dest = format!("{}/{}", self.dir, self.input);
        let src = self
            .entries
            .get(self.selected)
            .map(|(n, _)| format!("{}/{}", self.dir, n));
        let mut copy_too_large = false;
        let result = volume(|fs| match mode {
            Mode::File | Mode::Folder => fs.create_path(&dest, mode == Mode::Folder).map(|_| ()),
            Mode::Rename => fs.rename(src.as_deref().ok_or(FsError::FileNotFound)?, &self.input),
            Mode::Copy => {
                let id = fs.resolve(src.as_deref().ok_or(FsError::FileNotFound)?)?;
                // Sparse files may have a logical size far larger than RAM.
                // Until the filesystem supports streamed writes, bound copies.
                if fs.stat(id)?.0 > 256 * 1024 {
                    copy_too_large = true;
                    return Err(FsError::DiskFull);
                }
                let data = fs.read(id)?;
                let new = fs.create_path(&dest, false)?;
                if let Err(e) = fs.write(new, &data) {
                    fs.unlink(&dest)?;
                    return Err(e);
                }
                Ok(())
            }
            Mode::Delete => fs.unlink(src.as_deref().ok_or(FsError::FileNotFound)?),
            Mode::Browse => Ok(()),
        });
        if copy_too_large {
            self.mode = Mode::Browse;
            self.status = "Copy limit is 256 KiB".into();
            return;
        }
        self.refresh();
        if let Err(e) = result {
            self.status = e.to_string();
        } else if mode != Mode::Delete {
            if let Some(i) = self.entries.iter().position(|(n, _)| n == &self.input) {
                self.selected = i;
            }
        }
    }

    pub fn key(&mut self, key: Key) {
        if self.mode != Mode::Browse {
            match key {
                Key::Enter => self.submit(),
                Key::Escape => self.mode = Mode::Browse,
                Key::Backspace => {
                    self.input.pop();
                }
                Key::Char(b @ 32..=126) if self.input.len() < 56 && self.mode != Mode::Delete => {
                    self.input.push(b as char)
                }
                _ => {}
            }
            return;
        }
        match key {
            Key::Up => self.selected = self.selected.saturating_sub(1),
            Key::Down => {
                self.selected = (self.selected + 1).min(self.entries.len().saturating_sub(1))
            }
            Key::Enter => self.tool(1),
            Key::Backspace => self.tool(0),
            Key::F(2) => self.tool(4),
            Key::F(5) => self.refresh(),
            Key::Char(b'n') => self.tool(2),
            Key::Char(b'd') => self.tool(3),
            Key::Char(b'c') => self.tool(5),
            Key::Delete => self.tool(6),
            _ => {}
        }
    }

    fn buttons(w: &Window) -> [Rect; 7] {
        let cols = if w.w >= 550 { 7 } else { 4 };
        let width = (w.w - 24) / cols;
        core::array::from_fn(|i| {
            Rect::new(
                w.x + 12 + (i % cols * width) as i32,
                w.y + (TITLEBAR_H + 44 + i / cols * 28) as i32,
                width - 3,
                26,
            )
        })
    }
    fn list_geometry(&self, w: &Window) -> (i32, usize, usize) {
        let y = Self::buttons(w)[6].y + 57;
        let rows = ((w.y + w.h as i32 - 34 - y).max(0) / 24) as usize;
        (
            y,
            rows,
            self.selected.saturating_sub(rows.saturating_sub(1)),
        )
    }
    pub fn click(&mut self, w: &Window, x: i32, y: i32) {
        if self.mode != Mode::Browse {
            return;
        }
        if let Some(i) = Self::buttons(w).iter().position(|r| r.contains(x, y)) {
            self.tool(i);
            return;
        }
        let (top, rows, first) = self.list_geometry(w);
        if y >= top && y < top + rows as i32 * 24 {
            let i = first + ((y - top) / 24) as usize;
            if i < self.entries.len() {
                self.selected = i;
            }
        }
    }

    pub fn wheel(&mut self, lines: i32) {
        if self.mode == Mode::Browse {
            self.selected = (self.selected as i32 - lines * 3)
                .clamp(0, self.entries.len().saturating_sub(1) as i32)
                as usize;
        }
    }
    pub fn draw(&self, f: &mut [u32], stride: usize, h: usize, w: &Window) {
        let left = w.x + 12;
        let right = w.x + w.w as i32 - 12;
        let path_y = w.y + TITLEBAR_H as i32 + 6;
        rounded_rect(f, stride, h, left, path_y, w.w - 24, 32, 8, SEARCH_BG);
        draw_text(
            f,
            stride,
            h,
            left + 8,
            path_y + 6,
            left + 100,
            "Zenith FS   /",
            ACCENTS[1],
        );
        draw_text(
            f,
            stride,
            h,
            left + 104,
            path_y + 6,
            right,
            if self.dir.is_empty() {
                "Home"
            } else {
                self.dir.trim_start_matches('/')
            },
            FG,
        );
        for (i, (r, name)) in Self::buttons(w).iter().zip(TOOLS).enumerate() {
            let color = match i {
                2 | 3 => blend(MENU_TILE, ACCENTS[1], 27),
                6 => blend(MENU_TILE, ACCENTS[3], 26),
                _ => MENU_TILE,
            };
            rounded_rect(f, stride, h, r.x, r.y, r.w, r.h, 6, color);
            draw_text_centered(f, stride, h, r.x + r.w as i32 / 2, r.y + 3, name, MUTED);
        }
        let (top, rows, first) = self.list_geometry(w);
        draw_text(f, stride, h, left + 8, top - 24, right, "Name", DIM);
        draw_text(f, stride, h, right - 76, top - 24, right, "Kind", DIM);
        fill(f, stride, h, left, top - 2, w.w - 24, 1, CARD_LINE);
        if rows >= 3 && self.entries.is_empty() && self.mode == Mode::Browse {
            let mid = top + (rows as i32 * 24) / 2;
            let mounted = self.status == "0 entries";
            draw_text_centered(
                f,
                stride,
                h,
                (left + right) / 2,
                mid - 23,
                if mounted {
                    "This folder is empty"
                } else {
                    "Storage unavailable"
                },
                FG,
            );
            draw_text_centered(
                f,
                stride,
                h,
                (left + right) / 2,
                mid + 3,
                if mounted {
                    "Use + File or + Folder to get started."
                } else {
                    "Attach a Zenith FS disk to browse files."
                },
                MUTED,
            );
        }
        for (j, (name, folder)) in self.entries.iter().skip(first).take(rows).enumerate() {
            let y = top + j as i32 * 24;
            if first + j == self.selected {
                rounded_rect(f, stride, h, left, y, w.w - 24, 24, 4, MENU_TILE_SELECTED);
            } else if j % 2 == 1 {
                rounded_rect(f, stride, h, left, y, w.w - 24, 24, 4, CARD_BG);
            }
            if *folder {
                rounded_rect(f, stride, h, left + 8, y + 5, 9, 5, 2, 0xefd099);
                rounded_rect(f, stride, h, left + 8, y + 8, 19, 12, 2, 0xd6af73);
            } else {
                rounded_rect(f, stride, h, left + 10, y + 4, 13, 16, 2, 0xaec9ec);
                fill(f, stride, h, left + 13, y + 10, 7, 1, 0x4b6987);
                fill(f, stride, h, left + 13, y + 14, 5, 1, 0x4b6987);
            }
            draw_text(
                f,
                stride,
                h,
                right - 76,
                y + 2,
                right,
                if *folder { "Folder" } else { "File" },
                MUTED,
            );
            draw_text(f, stride, h, left + 36, y + 2, right - 86, name, FG);
        }
        let prompt = match self.mode {
            Mode::Browse => self.status.as_str(),
            Mode::File => "New file:",
            Mode::Folder => "New folder:",
            Mode::Rename => "Rename to:",
            Mode::Copy => "Copy as:",
            Mode::Delete => "Delete selection? Enter: delete   Esc: cancel",
        };
        let y = w.y + w.h as i32 - 29;
        fill(f, stride, h, left, y - 4, w.w - 24, 1, CARD_LINE);
        draw_text(
            f,
            stride,
            h,
            left,
            y,
            right,
            prompt,
            if self.mode == Mode::Browse { MUTED } else { FG },
        );
        if self.mode != Mode::Browse && self.mode != Mode::Delete {
            draw_text(
                f,
                stride,
                h,
                left + ui_art::text_width(prompt) + 8,
                y,
                right,
                &self.input,
                ACCENTS[0],
            );
        }
    }
}
