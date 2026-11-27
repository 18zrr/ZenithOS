//! File Manager: cache the directory listing between input events, never do I/O
//! while painting a frame.
use super::*;
use crate::fs::volume;
use alloc::{
    format,
    string::{String, ToString},
};
use zenith_fs::FsError;
mod navigation;
mod transfer;
use navigation::Tab;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Browse,
    File,
    Folder,
    Rename,
    Copy,
    Delete,
    Edit,
    SaveAs,
    ConfirmClose,
    Properties,
    Search,
}

pub(super) struct Files {
    tab: Tab,
    other_tabs: [Option<Tab>; 4],
    slot: usize,
    marked: Vec<String>,
    transfer: Option<transfer::Transfer>,
    entries: Vec<(String, bool)>,
    selected: usize,
    last_click: Option<(usize, i32, i32, u64)>,
    editor_first: Option<usize>,
    mode: Mode,
    input: String,
    status: String,
    cursor: usize,
    original: String,
    name_input: String,
    anchor: Option<usize>,
    close_window: bool,
    closed: bool,
}

const TOOLS: [&str; 9] = [
    "Up", "Open", "+ File", "+ Folder", "Rename", "Copy", "Delete", "Edit", "Properties",
];
const NOTE_LIMIT: usize = 4096;

impl Files {
    pub const fn new() -> Self {
        Self {
            tab: Tab::new(),
            other_tabs: [const { None }; 4],
            slot: 0,
            marked: Vec::new(),
            transfer: None,
            entries: Vec::new(),
            selected: 0,
            last_click: None,
            editor_first: None,
            mode: Mode::Browse,
            input: String::new(),
            status: String::new(),
            cursor: 0,
            original: String::new(),
            name_input: String::new(),
            anchor: None,
            close_window: false,
            closed: false,
        }
    }

    pub fn dirty(&self) -> bool {
        matches!(self.mode, Mode::Edit | Mode::SaveAs | Mode::ConfirmClose) && self.input != self.original
    }

    pub fn request_close(&mut self, window: bool) -> bool {
        if self.dirty() {
            self.mode = Mode::ConfirmClose;
            self.close_window = window;
            false
        } else { true }
    }

    pub fn take_closed(&mut self) -> bool { core::mem::take(&mut self.closed) }

    fn selection(&self) -> core::ops::Range<usize> {
        let a = self.anchor.unwrap_or(self.cursor);
        a.min(self.cursor)..a.max(self.cursor)
    }

    fn erase_selection(&mut self) -> bool {
        let range = self.selection();
        self.anchor = None;
        if range.is_empty() { return false; }
        self.cursor = range.start;
        self.input.replace_range(range, "");
        true
    }

    fn save(&mut self, new_name: Option<&str>) -> bool {
        if new_name.is_some_and(|s| s.is_empty() || matches!(s,"."|"..") || s.contains(['/', '\0'])) {
            self.status = "Use a single non-empty filename".into();
            return false;
        }
        let name = new_name.unwrap_or(&self.entries[self.selected].0);
        let path = format!("{}/{}", self.tab.dir, name);
        let result = volume(|fs| {
            if new_name.is_some() { fs.create_with_data(&path, self.input.as_bytes()).map(|_| ()) }
            else { let id = fs.resolve(&path)?; fs.write(id,self.input.as_bytes()) }
        });
        match result {
            Ok(()) => {
                self.original.clear();
                self.original.push_str(&self.input);
                if let Some(name) = new_name {
                    let name = name.to_string();
                    self.tab.query.clear();
                    self.refresh();
                    let Some(index) = self.entries.iter().position(|(n,_)| n == &name) else {
                        self.status = "File saved; refresh the folder to reopen it".into();
                        return true;
                    };
                    self.selected = index;
                }
                self.mode = Mode::Edit;
                self.status = "Saved. Ctrl+S: save   Ctrl+Shift+S: Save As   Esc: close".into();
                true
            }
            Err(e) => {
                self.status = format!("Save failed: {e}. Draft retained.");
                crate::notifications::post("Editor", &self.status);
                false
            }
        }
    }

    pub fn refresh(&mut self) {
        self.last_click = None;
        self.marked.clear();
        let selected = self.entries.get(self.selected).map(|(n,_)| n.clone());
        self.mode = Mode::Browse;
        self.entries.clear();
        match volume(|fs| fs.list(&self.tab.dir)) {
            Ok(entries) => {
                self.set_listing(entries);
                if let Some(i) = self.entries.iter().position(|(n,_)| Some(n) == selected.as_ref()) {
                    self.selected = i;
                }
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
            let Some((parent, child)) = self.tab.dir.rsplit_once('/') else { return; };
            let child = child.to_string();
            self.navigate(parent.to_string());
            if let Some(i) = self.entries.iter().position(|(n,_)| n == &child) { self.selected = i; }
            return;
        }
        if index == 2 || index == 3 {
            self.mode = if index == 2 { Mode::File } else { Mode::Folder };
            return;
        }
        let Some((name, folder)) = self.entries.get(self.selected) else {
            return;
        };
        let path = format!("{}/{}", self.tab.dir, name);
        match index {
            1 if *folder => {
                self.navigate(path);
            }
            9 if !folder => {
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
                self.input.push_str(name);
                self.mode = Mode::Rename;
            }
            5 if !folder => {
                self.mode = Mode::Copy;
            }
            5 => self.status = "Select a file to copy".into(),
            6 => self.mode = Mode::Delete,
            1 | 7 if !folder => {
                if index == 1 && name.rsplit_once('.').is_some_and(|(_, ext)| {
                    !["txt", "md", "log", "cfg", "conf", "ini", "csv"].iter()
                        .any(|allowed| ext.eq_ignore_ascii_case(allowed))
                }) {
                    self.status = "No application for this file type".into();
                    return;
                }
                match volume(|fs| {
                    let id = fs.resolve(&path)?;
                    if fs.stat(id)?.1 { return Err(FsError::InvalidPath); }
                    fs.read_range(id, 0, NOTE_LIMIT + 1)
                }) {
                    Ok(data) if data.len() <= NOTE_LIMIT && data.iter().all(|b| matches!(b, b'\n' | 32..=126)) => {
                        self.input = String::from_utf8(data).unwrap();
                        self.cursor = if index == 1 { 0 } else { self.input.len() };
                        self.original.clear();
                        self.original.push_str(&self.input);
                        self.anchor = None;
                        self.mode = Mode::Edit;
                        self.editor_first = None;
                        self.status = "Ctrl+S: save   Ctrl+Shift+S: Save As   Esc: close".into();
                    }
                    Ok(_) => self.status = "Cannot open: requires ASCII text, LF, at most 4 KiB".into(),
                    Err(e) => self.status = format!("Cannot open file: {e}"),
                }
            }
            7 => self.status = "Select a text file to edit".into(),
            8 => {
                self.status = match volume(|fs| {
                    let id = fs.resolve(&path)?;
                    let (size, folder) = fs.stat(id)?;
                    let (mode, uid, gid) = fs.permissions(id)?;
                    Ok(format!("{} | {} bytes | mode {:03o} | owner {}:{}",
                        if folder { "Folder" } else { "File" },size,mode,uid,gid))
                }) {
                    Ok(s) => { self.mode = Mode::Properties; s },
                    Err(e) => e.to_string(),
                };
            }
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
            self.status = "Use a single non-empty name".into();
            return;
        }
        let dest = format!("{}/{}", self.tab.dir, self.input);
        let src = self
            .entries
            .get(self.selected)
            .map(|(n, _)| format!("{}/{}", self.tab.dir, n));
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
                fs.create_with_data(&dest, &data)?;
                Ok(())
            }
            Mode::Delete => fs.unlink(src.as_deref().ok_or(FsError::FileNotFound)?),
            _ => Ok(()),
        });
        if copy_too_large {
            self.mode = Mode::Browse;
            self.status = "Copy limit is 256 KiB".into();
            return;
        }
        if let Err(e) = result {
            self.status = format!("Operation failed: {e}. Change the name or Esc to cancel.");
            crate::notifications::post("Explorer", &self.status);
            return;
        }
        if mode != Mode::Delete { self.tab.query.clear(); }
        self.refresh();
        if matches!(mode, Mode::Rename | Mode::Delete) { self.transfer = None; }
        crate::notifications::post("Explorer", match mode {
            Mode::File => "File created", Mode::Folder => "Folder created", Mode::Delete => "Entry deleted",
            Mode::Rename => "Entry renamed", Mode::Copy => "File copied", _ => "Operation completed",
        });
        if mode != Mode::Delete {
            if let Some(i) = self.entries.iter().position(|(n, _)| n == &self.input) {
                self.selected = i;
            }
        }
    }

    pub fn key(&mut self, event: KeyEvent, w: &Window) {
        self.last_click = None;
        self.editor_first = None;
        self.handle_key(event, w);
        if self.mode == Mode::Browse { self.reveal_selection(&self.pane(w)); }
    }

    fn handle_key(&mut self, event: KeyEvent, w: &Window) {
        let key = event.key;
        if self.mode == Mode::Search {
            match key {
                Key::Escape => self.mode = Mode::Browse,
                Key::Enter => {
                    self.tab.query.clone_from(&self.input);
                    self.refresh();
                }
                Key::Backspace => { self.input.pop(); }
                Key::Char(b'a') if event.ctrl => self.input.clear(),
                Key::Char(b @ 32..=126) if !event.ctrl && self.input.len() < 56 => self.input.push(b as char),
                _ => {}
            }
            return;
        }
        if self.mode == Mode::ConfirmClose {
            match key {
                Key::Escape | Key::Char(b'c') => self.mode = Mode::Edit,
                Key::Char(b's') => {
                    if self.save(None) {
                        if self.close_window { self.closed = true; }
                        self.mode = Mode::Browse;
                    }
                }
                Key::Char(b'd') => {
                    self.original.clear();
                    self.input.clear();
                    if self.close_window { self.closed = true; }
                    self.mode = Mode::Browse;
                }
                _ => {}
            }
            return;
        }
        if self.mode == Mode::SaveAs {
            match key {
                Key::Escape => self.mode = Mode::Edit,
                Key::Enter => {
                    let name = self.name_input.clone();
                    self.save(Some(&name));
                }
                Key::Backspace => { self.name_input.pop(); }
                Key::Char(b @ 32..=126) if !event.ctrl && self.name_input.len() < 56 => self.name_input.push(b as char),
                _ => {}
            }
            return;
        }
        if self.mode == Mode::Properties {
            if matches!(key,Key::Enter | Key::Escape) { self.mode = Mode::Browse; }
            return;
        }
        if self.mode == Mode::Edit {
            if event.ctrl && matches!(key, Key::Char(b'c' | b'x')) {
                let selection = self.selection();
                if !selection.is_empty() {
                    crate::clipboard::set(&self.input[selection]);
                    if key == Key::Char(b'x') { self.erase_selection(); }
                }
                return;
            }
            if event.ctrl && key == Key::Char(b'v') {
                let text = crate::clipboard::text();
                if !text.is_empty() && self.input.len() - self.selection().len() + text.len() <= NOTE_LIMIT {
                    self.erase_selection();
                    self.input.insert_str(self.cursor, &text);
                    self.cursor += text.len();
                } else if !text.is_empty() {
                    self.status = "Paste would exceed 4 KiB".into();
                }
                return;
            }
            if event.ctrl && matches!(key, Key::Char(b'a' | b'A')) {
                self.anchor = Some(0);
                self.cursor = self.input.len();
                return;
            }
            let movement = matches!(key,Key::Left|Key::Right|Key::Up|Key::Down|Key::Home|Key::End);
            if movement {
                if event.shift { self.anchor.get_or_insert(self.cursor); } else { self.anchor = None; }
            }
            if event.ctrl && matches!(key, Key::Char(b's' | b'S')) {
                if event.shift { self.name_input.clear(); self.mode = Mode::SaveAs; }
                else { self.save(None); }
            } else {
                match key {
                    Key::Escape => {
                        if self.request_close(false) { self.mode = Mode::Browse; }
                    }
                    Key::Left => self.cursor = self.cursor.saturating_sub(1),
                    Key::Right => self.cursor = (self.cursor + 1).min(self.input.len()),
                    Key::Up | Key::Down => {
                        let start = self.input[..self.cursor].rfind('\n').map_or(0, |n| n+1);
                        let column = self.cursor - start;
                        if key == Key::Up && start > 0 {
                            let previous = self.input[..start-1].rfind('\n').map_or(0, |n| n+1);
                            self.cursor = previous + column.min(start-1-previous);
                        } else if key == Key::Down {
                            if let Some(end) = self.input[self.cursor..].find('\n') {
                                let next = self.cursor + end + 1;
                                let line = self.input[next..].split('\n').next().unwrap_or("");
                                self.cursor = next + column.min(line.len());
                            }
                        }
                    }
                    Key::Home => self.cursor = self.input[..self.cursor].rfind('\n').map_or(0, |n| n+1),
                    Key::End => self.cursor += self.input[self.cursor..].find('\n').unwrap_or(self.input.len()-self.cursor),
                    Key::Backspace => {
                        if !self.erase_selection() && self.cursor > 0 { self.cursor -= 1; self.input.remove(self.cursor); }
                    }
                    Key::Delete => {
                        if !self.erase_selection() && self.cursor < self.input.len() { self.input.remove(self.cursor); }
                    }
                    Key::Enter | Key::Char(32..=126) if !event.ctrl && !event.alt => {
                        if self.input.len() - self.selection().len() < NOTE_LIMIT {
                            self.erase_selection();
                            let c = if let Key::Char(b) = key { b as char } else { '\n' };
                            self.input.insert(self.cursor, c);
                            self.cursor += 1;
                        } else { self.status = "4 KiB limit reached   Ctrl+S: save   Esc: discard".into(); }
                    }
                    _ => {}
                }
            }
            return;
        }
        if self.mode != Mode::Browse {
            match key {
                Key::Enter => self.submit(),
                Key::Escape => self.mode = Mode::Browse,
                Key::Char(b'a') if event.ctrl => self.input.clear(),
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
            Key::Char(b' ') => self.toggle_mark(),
            Key::Char(b'c' | b'x') if event.ctrl => self.copy_selection(key == Key::Char(b'x')),
            Key::Char(b'v') if event.ctrl => self.paste_selection(),
            Key::Left if event.alt => self.travel(false),
            Key::Right if event.alt => self.travel(true),
            Key::Char(b'f') if event.ctrl => self.search(),
            Key::Char(b'g') if event.ctrl => self.tab.grid = !self.tab.grid,
            Key::Char(b't') if event.ctrl => self.new_tab(),
            Key::Char(b'w') if event.ctrl => self.close_tab(),
            Key::Tab if event.ctrl => self.next_tab(event.shift),
            Key::Escape if !self.tab.query.is_empty() => { self.tab.query.clear(); self.refresh(); }
            Key::Home => self.selected = 0,
            Key::End => self.selected = self.entries.len().saturating_sub(1),
            Key::PageUp => self.selected = self.selected.saturating_sub(self.list_geometry(&self.pane(w)).1.max(1)),
            Key::PageDown => self.selected = (self.selected + self.list_geometry(&self.pane(w)).1.max(1)).min(self.entries.len().saturating_sub(1)),
            Key::Up => self.selected = self.selected.saturating_sub(self.columns(&self.pane(w))),
            Key::Left => self.selected = self.selected.saturating_sub(1),
            Key::Right | Key::Down => {
                let step = if key == Key::Right { 1 } else { self.columns(&self.pane(w)) };
                self.selected = (self.selected + step).min(self.entries.len().saturating_sub(1))
            }
            Key::Enter => self.tool(1),
            Key::Backspace => self.tool(0),
            Key::F(2) => self.tool(4),
            Key::F(5) => self.refresh(),
            Key::Char(b'n') => self.tool(2),
            Key::Char(b'd') => self.tool(3),
            Key::Char(b'c') => self.tool(5),
            Key::Char(b'e') => self.tool(7),
            Key::Char(b'p') => self.tool(8),
            Key::Char(b'v') => self.tool(9),
            Key::Delete => self.tool(6),
            _ => {}
        }
    }

    fn buttons(w: &Window) -> [Rect; TOOLS.len()] {
        let cols = if w.w >= 800 { TOOLS.len() } else if w.w >= 450 { 5 } else { 3 };
        let width = (w.w - 24) / cols;
        core::array::from_fn(|i| {
            Rect::new(
                w.x + 12 + (i % cols * width) as i32,
                w.y + (TITLEBAR_H + 104 + i / cols * 28) as i32,
                width - 3,
                26,
            )
        })
    }
    fn list_geometry(&self, w: &Window) -> (i32, usize, usize) {
        let y = if w.h < 325 { w.y + TITLEBAR_H as i32 + 124 }
            else { Self::buttons(w)[TOOLS.len() - 1].y + 57 };
        let rows = ((w.y + w.h as i32 - 30 - y).max(0) / self.cell_height()) as usize;
        let cols = self.columns(w);
        (
            y,
            rows * cols,
            self.tab.first_row.min(self.entries.len().div_ceil(cols).saturating_sub(rows)) * cols,
        )
    }
    fn reveal_selection(&mut self, w: &Window) {
        let (_, capacity, first) = self.list_geometry(w);
        let cols = self.columns(w);
        let row = self.selected / cols;
        let rows = (capacity / cols).max(1);
        self.tab.first_row = (first / cols).clamp((row + 1).saturating_sub(rows), row);
    }
    fn columns(&self, w: &Window) -> usize {
        if self.tab.grid { ((w.w - 24) / 132).max(1) } else { 1 }
    }
    fn cell_height(&self) -> i32 { if self.tab.grid { 44 } else { 24 } }
    pub fn click(&mut self, w: &Window, x: i32, y: i32) {
        let previous = self.last_click.take();
        if self.mode != Mode::Browse {
            return;
        }
        let mut target = None;
        self.sidebar(w, |r, _, path| if r.contains(x,y) { target = Some(path.to_string()); });
        if let Some(path) = target { self.navigate(path); return; }
        let pane = self.pane(w);
        if x < pane.x + 12 || x >= pane.x + pane.w as i32 - 12 { return; }
        let w = &pane;
        if self.navigation_click(w, x, y) { return; }
        if let Some(i) = Self::buttons(w).iter().position(|r| w.h >= 325 && r.contains(x, y)) {
            self.tool(i);
            if self.mode == Mode::Browse { self.reveal_selection(w); }
            return;
        }
        let (top, rows, first) = self.list_geometry(w);
        if y >= top - 27 && y < top - 2 {
            self.tab.reverse = !self.tab.reverse;
            self.refresh();
            return;
        }
        let cols = self.columns(w);
        let cell_w = (w.w - 24) / cols;
        if y >= top && y < top + (rows / cols) as i32 * self.cell_height() {
            let col = (x - w.x - 12) as usize / cell_w;
            if col >= cols { return; }
            let i = first + ((y - top) / self.cell_height()) as usize * cols + col;
            if i < self.entries.len() {
                self.selected = i;
                let now = timer::ticks();
                if previous.is_some_and(|(entry, px, py, time)| {
                    entry == i && now.wrapping_sub(time) <= 50
                        && x.abs_diff(px) <= 4 && y.abs_diff(py) <= 4
                }) {
                    self.tool(1);
                } else {
                    self.last_click = Some((i, x, y, now));
                }
            }
        }
    }

    pub fn cancel_click(&mut self) { self.last_click = None; }

    pub fn wheel(&mut self, window: &Window, x: i32, y: i32, lines: i32) -> bool {
        self.last_click = None;
        let w = self.pane(window);
        let (first, max) = match self.mode {
            Mode::Browse => {
                let (top, capacity, first) = self.list_geometry(&w);
                let cols = self.columns(&w);
                if x < w.x + 12 || y < top || y >= w.y + w.h as i32 - 30 || capacity == 0 {
                    return false;
                }
                (first / cols, self.entries.len().div_ceil(cols).saturating_sub(capacity / cols))
            }
            Mode::Edit => {
                let (cols, rows) = Self::editor_geometry(&w);
                let last = Self::text_position(&self.input, cols).0;
                let cursor = Self::text_position(&self.input[..self.cursor], cols).0;
                let max = (last + 1).saturating_sub(rows);
                (self.editor_first.unwrap_or((cursor + 1).saturating_sub(rows)).min(max), max)
            }
            _ => return false,
        };
        let row = (first as i32 - lines * 3).clamp(0, max as i32) as usize;
        if row == first { return false; }
        if self.mode == Mode::Edit { self.editor_first = Some(row); } else { self.tab.first_row = row; }
        true
    }

    fn editor_geometry(w: &Window) -> (usize, usize) {
        ((w.w.saturating_sub(24) / COL_PITCH).max(1),
         (w.h.saturating_sub(TITLEBAR_H + 76) / ROW_PITCH).max(1))
    }

    fn advance(row: &mut usize, col: &mut usize, b: u8, cols: usize) {
        if b == b'\n' || *col + 1 == cols { *row += 1; *col = 0; }
        else { *col += 1; }
    }

    fn text_position(text: &str, cols: usize) -> (usize, usize) {
        let (mut row, mut col) = (0, 0);
        for b in text.bytes() { Self::advance(&mut row, &mut col, b, cols); }
        (row, col)
    }
    pub fn draw(&self, f: &mut [u32], stride: usize, h: usize, w: &Window) {
        self.sidebar(w, |r, name, _| {
            rounded_rect(f,stride,h,r.x,r.y,r.w,r.h,5,CARD_BG);
            draw_text(f,stride,h,r.x+6,r.y+4,r.right()-4,name,FG);
        });
        let pane = self.pane(w);
        if pane.x != w.x { draw_text(f,stride,h,w.x+12,w.y+TITLEBAR_H as i32+12,pane.x,"Locations",MUTED); }
        let w = &pane;
        let left = w.x + 12;
        let right = w.x + w.w as i32 - 12;
        let path_y = w.y + TITLEBAR_H as i32 + 6;
        if matches!(self.mode,Mode::ConfirmClose|Mode::SaveAs|Mode::Properties|Mode::Delete) {
            let width = w.w.saturating_sub(32);
            let x = w.x+16;
            let y = w.y+TITLEBAR_H as i32+24;
            rounded_rect(f,stride,h,x,y,width,156,10,TITLE_ACTIVE);
            let title = match self.mode {
                Mode::ConfirmClose => "Unsaved document",
                Mode::SaveAs => "Save As - new filename",
                Mode::Properties => "Properties",
                _ => "Delete selected entry?",
            };
            draw_text(f,stride,h,x+12,y+12,right,title,FG);
            draw_text(f,stride,h,x+12,y+40,right,
                self.entries.get(self.selected).map_or("", |(n,_)| n.as_str()),ACCENTS[0]);
            let hint = match self.mode {
                Mode::ConfirmClose => "S: save and close   D: discard   Esc: cancel",
                Mode::SaveAs => "Enter: save new file   Esc: return to editor",
                Mode::Properties => "Enter / Esc: return",
                _ => "Enter: delete   Esc: cancel",
            };
            if self.mode == Mode::SaveAs {
                draw_text(f,stride,h,x+12,y+70,right,&self.name_input,FG);
            } else if self.mode == Mode::Properties {
                // Each field gets a row so narrow windows do not hide values.
                for (i,field) in self.status.split(" | ").enumerate() {
                    draw_text(f,stride,h,x+12,y+66+i as i32*20,right,field,MUTED);
                }
            }
            draw_text(f,stride,h,x+12,y+170,right,hint,MUTED);
            if self.mode != Mode::Properties {
                draw_text(f,stride,h,left,w.y+w.h as i32-29,right,&self.status,MUTED);
            }
            return;
        }
        if self.mode == Mode::Edit {
            if self.dirty() { draw_text(f,stride,h,left,path_y,right,"*",ACCENTS[0]); }
            draw_text(f, stride, h, left+16, path_y, right, &self.entries[self.selected].0, FG);
            let (cols, rows) = Self::editor_geometry(w);
            let (cursor_row, cursor_col) = Self::text_position(&self.input[..self.cursor], cols);
            let first = self.editor_first.map_or_else(|| (cursor_row + 1).saturating_sub(rows),
                |r| r.min((Self::text_position(&self.input, cols).0 + 1).saturating_sub(rows)));
            let text_y = path_y + 30;
            let (mut row, mut col) = (0, 0);
            let selection = self.selection();
            for (index,&b) in self.input.as_bytes().iter().enumerate() {
                if row >= first + rows { break; }
                if row >= first && b != b'\n' {
                    let x = left + (col * COL_PITCH) as i32;
                    let y = text_y + ((row - first) * ROW_PITCH) as i32;
                    if x >= 0 && y >= 0 {
                        if selection.contains(&index) {
                            fill(f,stride,h,x,y,COL_PITCH,ROW_PITCH,MENU_TILE_SELECTED);
                        }
                        fb::draw_glyph_to(f, stride, h, x as usize, y as usize, b, FG);
                    }
                }
                Self::advance(&mut row, &mut col, b, cols);
            }
            if (first..first + rows).contains(&cursor_row) {
                fill(f,stride,h,left + (cursor_col * COL_PITCH) as i32,
                     text_y + ((cursor_row - first) * ROW_PITCH) as i32,1,ROW_PITCH,ACCENTS[0]);
            }
            draw_text(f,stride,h,left,w.y+w.h as i32-29,right,&self.status,MUTED);
            return;
        }
        self.draw_navigation(f, stride, h, w);
        for (i, (r, name)) in Self::buttons(w).iter().zip(TOOLS).enumerate() {
            if w.h < 325 { break; }
            let (mx,my) = mouse::position();
            let color = if r.contains(mx,my) {
                if mouse::buttons() & mouse::BUTTON_LEFT != 0 { MENU_TILE_SELECTED } else { MENU_TILE_HOVER }
            } else { match i {
                2 | 3 => blend(MENU_TILE, ACCENTS[1], 27),
                6 => blend(MENU_TILE, ACCENTS[3], 26),
                _ => MENU_TILE,
            }};
            rounded_rect(f, stride, h, r.x, r.y, r.w, r.h, 6, color);
            draw_text_centered(f, stride, h, r.x + r.w as i32 / 2, r.y + 3, name, MUTED);
        }
        let (top, rows, first) = self.list_geometry(w);
        draw_text(f, stride, h, left + 8, top - 24, right,
            if self.tab.reverse { "Name (Z-A)" } else { "Name (A-Z)" }, DIM);
        if !self.tab.grid { draw_text(f, stride, h, right - 76, top - 24, right, "Kind", DIM); }
        fill(f, stride, h, left, top - 2, w.w - 24, 1, CARD_LINE);
        if rows >= 3 && self.entries.is_empty() && self.mode == Mode::Browse {
            let mid = top + (rows / self.columns(w)) as i32 * self.cell_height() / 2;
            let mounted = self.status == "0 entries";
            draw_text_centered(
                f,
                stride,
                h,
                (left + right) / 2,
                mid - 23,
                if !self.tab.query.is_empty() { "No matching entries" } else if mounted {
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
                if !self.tab.query.is_empty() { "Esc: clear search" } else if mounted {
                    "Use + File or + Folder to get started."
                } else {
                    "Attach a Zenith FS disk to browse files."
                },
                MUTED,
            );
        }
        for (j, (name, folder)) in self.entries.iter().skip(first).take(rows).enumerate() {
            let cols = self.columns(w);
            let width = (w.w - 24) / cols;
            let left = left + (j % cols * width) as i32;
            let right = left + width as i32;
            let y = top + (j / cols) as i32 * self.cell_height();
            if first + j == self.selected {
                rounded_rect(f, stride, h, left, y, width, self.cell_height() as usize, 4, MENU_TILE_SELECTED);
            } else if j % 2 == 1 {
                rounded_rect(f, stride, h, left, y, width, self.cell_height() as usize, 4, CARD_BG);
            }
            if self.marked.contains(name) { fill(f,stride,h,left,y+2,3,20,ACCENTS[1]); }
            if *folder {
                rounded_rect(f, stride, h, left + 8, y + 5, 9, 5, 2, 0xefd099);
                rounded_rect(f, stride, h, left + 8, y + 8, 19, 12, 2, 0xd6af73);
            } else {
                rounded_rect(f, stride, h, left + 10, y + 4, 13, 16, 2, 0xaec9ec);
                fill(f, stride, h, left + 13, y + 10, 7, 1, 0x4b6987);
                fill(f, stride, h, left + 13, y + 14, 5, 1, 0x4b6987);
            }
            if self.tab.grid {
                draw_text(f, stride, h, left + 8, y + 22, right - 4, name, FG);
                continue;
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
            Mode::Browse | Mode::Edit => self.status.as_str(),
            Mode::File => "New file:",
            Mode::Folder => "New folder:",
            Mode::Rename => "Rename to:",
            Mode::Copy => "Copy as:",
            Mode::Search => "Find in folder:",
            Mode::Delete => "Delete selection? Enter: delete   Esc: cancel",
            _ => self.status.as_str(),
        };
        let y = w.y + w.h as i32 - 29;
        if matches!(self.mode, Mode::File | Mode::Folder | Mode::Rename | Mode::Copy)
            && (self.status.starts_with("Operation failed:") || self.status.starts_with("Use a single")) {
            fill(f,stride,h,left,y-28,w.w-24,26,WINDOW_BG);
            draw_text(f,stride,h,left,y-26,right,&self.status,STATUS_WARN);
        }
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
