//! Directory tabs and bounded navigation history. Resolve a destination
//! before changing the active location, including when retracing history.
use super::*;

pub(super) struct Tab {
    pub(super) dir: String,
    pub(super) query: String,
    pub(super) reverse: bool,
    pub(super) grid: bool,
    back: Vec<String>,
    forward: Vec<String>,
    selection: usize,
    pub(super) first_row: usize,
}

impl Tab {
    pub(super) const fn new() -> Self {
        Self {
            dir: String::new(),
            query: String::new(),
            reverse: false,
            grid: false,
            back: Vec::new(),
            forward: Vec::new(),
            selection: 0,
            first_row: 0,
        }
    }
}

fn remember(history: &mut Vec<String>, location: String) {
    if history.len() == 32 {
        history.remove(0);
    }
    history.push(location);
}

impl Files {
    pub(super) fn set_listing(&mut self, mut entries: Vec<(String, bool)>) {
        let query = self.tab.query.as_bytes();
        if !query.is_empty() {
            entries.retain(|(name, _)| {
                name.as_bytes()
                    .windows(query.len())
                    .any(|s| s.eq_ignore_ascii_case(query))
            });
        }
        entries.sort_unstable_by(|a, b| {
            let names = a.0.cmp(&b.0);
            b.1.cmp(&a.1).then(if self.tab.reverse {
                names.reverse()
            } else {
                names
            })
        });
        self.entries = entries;
        self.status = if query.is_empty() {
            format!("{} entries", self.entries.len())
        } else {
            format!(
                "Find: {} | {} matches | Esc: clear",
                self.tab.query,
                self.entries.len()
            )
        };
    }

    fn change_dir(&mut self, path: String) -> Option<String> {
        match volume(|fs| fs.list(&path)) {
            Ok(entries) => {
                self.marked.clear();
                let previous = core::mem::replace(&mut self.tab.dir, path);
                self.tab.query.clear();
                self.selected = 0;
                self.tab.first_row = 0;
                self.set_listing(entries);
                Some(previous)
            }
            Err(e) => {
                self.status = format!("Cannot open folder: {e}");
                None
            }
        }
    }

    pub(super) fn navigate(&mut self, path: String) {
        if path == self.tab.dir {
            return;
        }
        if let Some(old) = self.change_dir(path) {
            remember(&mut self.tab.back, old);
            self.tab.forward.clear();
        }
    }

    pub(super) fn travel(&mut self, forward: bool) {
        let history = if forward {
            &self.tab.forward
        } else {
            &self.tab.back
        };
        let Some(path) = history.last().cloned() else {
            return;
        };
        if let Some(old) = self.change_dir(path) {
            if forward {
                self.tab.forward.pop();
                remember(&mut self.tab.back, old);
            } else {
                self.tab.back.pop();
                remember(&mut self.tab.forward, old);
            }
        }
    }

    pub(super) fn search(&mut self) {
        self.mode = Mode::Search;
        self.input.clone_from(&self.tab.query);
    }

    fn switch_tab(&mut self, slot: usize) {
        if let Some(new) = self.other_tabs[slot].take() {
            self.tab.selection = self.selected;
            self.other_tabs[self.slot] = Some(core::mem::replace(&mut self.tab, new));
            self.slot = slot;
            self.selected = self.tab.selection;
            self.entries.clear();
            self.refresh();
        }
    }

    pub(super) fn new_tab(&mut self) {
        let Some(i) = (0..4).find(|&i| i != self.slot && self.other_tabs[i].is_none()) else {
            self.status = "Four tabs are already open".into();
            return;
        };
        self.open_slot(i);
    }

    fn open_slot(&mut self, i: usize) {
        let mut tab = Tab::new();
        tab.dir.clone_from(&self.tab.dir);
        self.other_tabs[i] = Some(tab);
        self.switch_tab(i);
    }

    pub(super) fn next_tab(&mut self, back: bool) {
        for offset in 1..4 {
            let i = if back {
                (self.slot + 4 - offset) % 4
            } else {
                (self.slot + offset) % 4
            };
            if self.other_tabs[i].is_some() {
                self.switch_tab(i);
                return;
            }
        }
    }

    pub(super) fn close_tab(&mut self) {
        let old = self.slot;
        self.next_tab(true);
        if self.slot != old {
            self.other_tabs[old] = None;
        }
    }

    fn nav_rect(window: &Window, row: usize, index: usize, columns: usize) -> Rect {
        let width = (window.w - 24) / columns;
        Rect::new(
            window.x + 12 + (index * width) as i32,
            window.y + (TITLEBAR_H + 6 + row * 31) as i32,
            width - 3,
            26,
        )
    }

    // Visit visible breadcrumb targets from leaf to root. Painting and input
    // use identical clipping, including at the narrowest window size.
    fn crumbs(&self, w: &Window, mut visit: impl FnMut(Rect, &str, &str)) {
        let root = Rect::new(w.x + 12, w.y + TITLEBAR_H as i32 + 68, 88, 26);
        visit(root, "Zenith FS", "");
        let mut right = w.x + w.w as i32 - 12;
        let mut path = self.tab.dir.as_str();
        while let Some((parent, name)) = path.rsplit_once('/') {
            let available = right - root.right() - 4;
            if available < 30 {
                break;
            }
            let width = (ui_art::text_width(name) + 18).min(available) as usize;
            let rect = Rect::new(right - width as i32, root.y, width, 26);
            visit(rect, name, path);
            right = rect.x - 4;
            path = parent;
        }
    }

    pub(super) fn navigation_click(&mut self, w: &Window, x: i32, y: i32) -> bool {
        for i in 0..4 {
            if Self::nav_rect(w, 0, i, 4).contains(x, y) {
                if i != self.slot && self.other_tabs[i].is_none() {
                    self.open_slot(i);
                } else {
                    self.switch_tab(i);
                }
                return true;
            }
        }
        for i in 0..5 {
            if Self::nav_rect(w, 1, i, 5).contains(x, y) {
                match i {
                    0 => self.travel(false),
                    1 => self.travel(true),
                    2 => self.search(),
                    3 => self.tab.grid = !self.tab.grid,
                    _ => self.close_tab(),
                }
                return true;
            }
        }
        let mut target = None;
        self.crumbs(w, |r, _, path| {
            if r.contains(x, y) {
                target = Some(path.to_string());
            }
        });
        if let Some(path) = target {
            self.navigate(path);
            return true;
        }
        false
    }

    pub(super) fn draw_navigation(&self, f: &mut [u32], stride: usize, h: usize, w: &Window) {
        let (mx, my) = mouse::position();
        for i in 0..4 {
            let tab = if self.slot == i {
                Some(&self.tab)
            } else {
                self.other_tabs[i].as_ref()
            };
            let label = tab.map_or("+", |t| {
                t.dir
                    .rsplit('/')
                    .next()
                    .filter(|n| !n.is_empty())
                    .unwrap_or("Home")
            });
            let r = Self::nav_rect(w, 0, i, 4);
            rounded_rect(
                f,
                stride,
                h,
                r.x,
                r.y,
                r.w,
                r.h,
                5,
                if i == self.slot {
                    MENU_TILE_SELECTED
                } else {
                    MENU_TILE
                },
            );
            draw_text(f, stride, h, r.x + 6, r.y + 3, r.right() - 5, label, FG);
        }
        for (i, label) in [
            "Back",
            "Forward",
            "Find",
            if self.tab.grid { "List" } else { "Grid" },
            "Close",
        ]
        .iter()
        .enumerate()
        {
            let r = Self::nav_rect(w, 1, i, 5);
            draw_button(
                f,
                stride,
                h,
                &r,
                label,
                false,
                r.contains(mx, my),
                ACCENTS[0],
            );
        }
        self.crumbs(w, |r, label, _| {
            rounded_rect(f, stride, h, r.x, r.y, r.w, r.h, 5, SEARCH_BG);
            draw_text(
                f,
                stride,
                h,
                r.x + 5,
                r.y + 3,
                r.right() - 4,
                label,
                ACCENTS[1],
            );
        });
    }

    pub(super) fn pane(&self, w: &Window) -> Window {
        if w.w >= 700 && self.mode == Mode::Browse {
            Window {
                x: w.x + 132,
                w: w.w - 132,
                ..*w
            }
        } else {
            *w
        }
    }

    pub(super) fn sidebar(&self, w: &Window, mut visit: impl FnMut(Rect, &str, &str)) {
        if self.pane(w).x == w.x {
            return;
        }
        let rect = |i: usize| {
            Rect::new(
                w.x + 8,
                w.y + TITLEBAR_H as i32 + 42 + i as i32 * 32,
                122,
                28,
            )
        };
        visit(rect(0), "Zenith FS  /", "");
        let max_rows = w.h.saturating_sub(TITLEBAR_H + 96) / 32;
        for (i, path) in self
            .tab
            .back
            .iter()
            .rev()
            .filter(|p| !p.is_empty())
            .take(max_rows.min(6))
            .enumerate()
        {
            visit(rect(i + 1), path.rsplit('/').next().unwrap_or(""), path);
        }
    }
}
