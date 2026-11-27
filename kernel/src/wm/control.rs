use super::*;

const ACTIONS: [&str; 7] = [
    "Settings",
    "Display",
    "Log out",
    "Restart",
    "Shut down",
    "Dismiss latest",
    "Clear history",
];

fn control_button(w: &Window, i: usize) -> Rect {
    let width = (w.w - 38) / 2;
    Rect::new(
        w.x + 16 + (i % 2 * (width + 6)) as i32,
        w.y + TITLEBAR_H as i32 + 124 + (i / 2 * 32) as i32,
        width,
        26,
    )
}

impl Wm {
    pub(super) fn control_click(&mut self, w: &Window, x: i32, y: i32) -> Action {
        for i in 0..ACTIONS.len() {
            if !control_button(w, i).contains(x, y) {
                continue;
            }
            match i {
                0 | 1 => {
                    self.settings_page = if i == 0 { Page::System } else { Page::Display };
                    self.display_input_active = false;
                    self.open_window(WindowKind::Settings);
                }
                2..=4 => return self.session_action(i - 2),
                5 => crate::notifications::dismiss(),
                _ => crate::notifications::clear(),
            }
            invalidate();
            break;
        }
        Action::None
    }

    pub(super) fn draw_control(&self, frame: &mut [u32], w: &Window, mx: i32, my: i32) {
        let height = frame.len() / self.width;
        let x = w.x + 16;
        let y = w.y + TITLEBAR_H as i32 + 12;
        let right = w.x + w.w as i32 - 24;
        self.card(frame, x, y, w.w - 32, 104, env!("ZENITH_RELEASE_ASCII"));
        self.monitor_row(
            frame,
            x + 12,
            y + 40,
            right,
            "Network",
            if e1000::mac() == [0; 6] {
                "No interface"
            } else if e1000::link_up() {
                "Link up"
            } else {
                "Link down"
            },
        );
        self.monitor_row(frame, x + 12, y + 68, right, "Audio volume", "Unsupported");
        for (i, label) in ACTIONS.iter().enumerate() {
            let r = control_button(w, i);
            draw_button(
                frame,
                self.width,
                height,
                &r,
                label,
                false,
                r.contains(mx, my),
                self.accent,
            );
        }
        draw_text(
            frame,
            self.width,
            height,
            x,
            y + 250,
            right,
            "Notifications",
            self.accent,
        );
        let history = crate::notifications::snapshot();
        if history[0].source.is_empty() {
            draw_text(
                frame,
                self.width,
                height,
                x,
                y + 278,
                right,
                "No notifications this session",
                MUTED,
            );
        }
        for (i, notice) in history
            .iter()
            .take_while(|n| !n.source.is_empty())
            .enumerate()
        {
            let row = y + 278 + i as i32 * 40;
            let mut label = TextBuffer::new();
            label.push_str(notice.source);
            label.push_str("  +");
            label.push_dec(notice.second);
            label.push_str(" s");
            draw_text(
                frame,
                self.width,
                height,
                x,
                row,
                right,
                label.as_str(),
                DIM,
            );
            draw_text(
                frame,
                self.width,
                height,
                x,
                row + 18,
                right,
                notice.text(),
                FG,
            );
        }
    }
}
