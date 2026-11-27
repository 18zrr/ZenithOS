use super::*;
use crate::task::{self, IDLE_TASK, TASK_NAMES};

const SAMPLES: usize = 60;

pub(super) struct Monitor {
    previous: Option<[u64; TASK_NAMES.len()]>,
    next: u64,
    cpu: [u8; SAMPLES],
    heap: [u8; SAMPLES],
    count: usize,
    share: [u8; TASK_NAMES.len()],
    live: [bool; TASK_NAMES.len()],
    volume_kib: Option<(usize, usize)>,
    by_cpu: bool,
    query: [u8; 24],
    query_len: usize,
    searching: bool,
}

impl Monitor {
    pub(super) const fn new() -> Self {
        Self {
            previous: None,
            next: 0,
            cpu: [0; SAMPLES],
            heap: [0; SAMPLES],
            count: 0,
            share: [0; TASK_NAMES.len()],
            live: [false; TASK_NAMES.len()],
            volume_kib: None,
            by_cpu: false,
            query: [0; 24],
            query_len: 0,
            searching: false,
        }
    }

    pub(super) fn update(&mut self, now: u64, visible: bool, seconds: u8) -> bool {
        if !visible {
            self.previous = None;
            self.count = 0;
            return false;
        }
        if self.previous.is_some() && now < self.next {
            return false;
        }
        self.next = now + u64::from(seconds) * 100;
        let (runtime, live) = task::accounting();
        self.live = live;
        self.volume_kib = crate::fs::volume(|fs| {
            let sb = fs.superblock();
            Ok((
                sb.free_blocks * sb.block_size / 1024,
                sb.total_blocks * sb.block_size / 1024,
            ))
        })
        .ok();
        if let Some(previous) = self.previous.replace(runtime) {
            let elapsed: [u64; TASK_NAMES.len()] =
                core::array::from_fn(|i| runtime[i].saturating_sub(previous[i]));
            let total: u64 = elapsed.iter().sum();
            if total > 0 {
                self.share = elapsed.map(|v| ((u128::from(v) * 100) / u128::from(total)) as u8);
                self.cpu.rotate_left(1);
                self.heap.rotate_left(1);
                self.cpu[SAMPLES - 1] = ((total - elapsed[IDLE_TASK]) * 100 / total) as u8;
                self.heap[SAMPLES - 1] =
                    (crate::allocator::used() * 100 / crate::allocator::HEAP_SIZE) as u8;
                self.count = (self.count + 1).min(SAMPLES);
            }
        }
        true
    }
}

impl Wm {
    pub(super) fn draw_accounting(&self, frame: &mut [u32], window: &Window, y: i32) {
        let x = window.x + 16;
        let width = window.w.saturating_sub(32);
        let left = x + 14;
        let right = x + width as i32 - 14;
        let h = frame.len() / self.width;
        let m = &self.monitor;
        let title = if m.searching || m.query_len > 0 {
            "Find tasks:"
        } else if m.by_cpu {
            "Tasks by CPU | S: sort   Ctrl+F: find"
        } else {
            "Tasks by ID | S: sort   Ctrl+F: find"
        };
        self.card(frame, x, y, width, 212, title);
        if m.query_len > 0 {
            draw_text(
                frame,
                self.width,
                h,
                left + 96,
                y + 12,
                right,
                core::str::from_utf8(&m.query[..m.query_len]).unwrap_or(""),
                FG,
            );
        }
        let mut order: [usize; TASK_NAMES.len()] = core::array::from_fn(|i| i);
        if m.by_cpu {
            order.sort_unstable_by_key(|&i| (core::cmp::Reverse(m.share[i]), i));
        }
        let query = &m.query[..m.query_len];
        let visible = order.into_iter().filter(|&i| {
            query.is_empty()
                || TASK_NAMES[i]
                    .as_bytes()
                    .windows(query.len())
                    .any(|part| part.eq_ignore_ascii_case(query))
        });
        for (row, i) in visible.enumerate() {
            let name = TASK_NAMES[i];
            let mut value = TextBuffer::new();
            if !m.live[i] {
                value.push_str("Exited");
            } else if m.count == 0 {
                value.push_str("Sampling...");
            } else {
                value.push_dec(m.share[i] as u64);
                value.push_str("% of interval");
            }
            let mut label = TextBuffer::new();
            label.push_dec(i as u64 + 1);
            label.push_str("  ");
            label.push_str(name);
            self.monitor_row(
                frame,
                left,
                y + 40 + row as i32 * 22,
                right,
                label.as_str(),
                value.as_str(),
            );
        }
        for (row, (samples, label, color)) in [
            (&m.cpu, "CPU busy", self.accent),
            (&m.heap, "Kernel heap", STATUS_OK),
        ]
        .iter()
        .enumerate()
        {
            let gy = y + 112 + row as i32 * 48;
            draw_text(frame, self.width, h, left, gy, right, label, MUTED);
            if m.count != 0 {
                let mut value = TextBuffer::new();
                value.push_dec(samples[SAMPLES - 1] as u64);
                value.push(b'%');
                draw_text(
                    frame,
                    self.width,
                    h,
                    left,
                    gy + 19,
                    right,
                    value.as_str(),
                    FG,
                );
            }
            let chart_x = left + 115;
            let chart_w = (right - chart_x).max(0) as usize;
            zenith_ui::chart::percentage_chart(
                frame,
                self.width,
                (chart_x, gy, chart_w, 34),
                &samples[SAMPLES - m.count..],
                SAMPLES,
                *color,
                SEARCH_BG,
            );
        }
        self.card(frame, x, y + 222, width, 122, "Mounted storage");
        let mut value = TextBuffer::new();
        if let Some((free, total)) = m.volume_kib {
            value.push_dec(free as u64);
            value.push_str(" / ");
            value.push_dec(total as u64);
            value.push_str(" KiB free");
        } else {
            value.push_str("Not mounted");
        }
        self.monitor_row(frame, left, y + 266, right, "Zenith FS v5", value.as_str());
        let (read, written) = crate::ata::sector_counts();
        let mut io = TextBuffer::new();
        io.push_dec(read);
        io.push_str(" / ");
        io.push_dec(written);
        self.monitor_row(
            frame,
            left,
            y + 296,
            right,
            "Sectors read / written",
            io.as_str(),
        );
    }
}

/// Handle task filtering and sorting only while the Monitor window has focus.
pub fn key(event: KeyEvent) -> bool {
    let mut wm = WM.lock();
    if !wm
        .top_visible()
        .and_then(|i| wm.windows[i])
        .is_some_and(|w| w.kind == WindowKind::Monitor)
    {
        return false;
    }
    let m = &mut wm.monitor;
    match event.key {
        Key::Char(b'f') if event.ctrl => {
            m.searching = true;
            m.query_len = 0;
        }
        Key::Escape if m.searching || m.query_len != 0 => {
            m.query_len = 0;
            m.searching = false;
        }
        Key::Enter if m.searching => m.searching = false,
        Key::Backspace if m.searching => m.query_len = m.query_len.saturating_sub(1),
        Key::Char(b @ 32..=126) if m.searching => {
            if m.query_len < m.query.len() {
                m.query[m.query_len] = b;
                m.query_len += 1;
            }
        }
        Key::Char(b's') if !event.ctrl && !event.alt => m.by_cpu = !m.by_cpu,
        _ => return false,
    }
    invalidate();
    true
}
