//! Desktop-only context actions; shared keyboard and pointer hit geometry.
use super::*;
const ITEMS: [&str; 5] = ["Open Terminal", "Open File Manager", "Settings", "About ZenithOS", "Refresh desktop"];

impl Wm {
    pub(super) fn context_click(&mut self, x: i32, y: i32) {
        let Some((left, top, _)) = self.context.take() else { return; };
        if Rect::new(left, top, 220, 166).contains(x,y) {
            let index = ((y-top-8).max(0) / 30) as usize;
            self.context_action(index);
        }
        invalidate();
    }

    fn context_action(&mut self, index: usize) {
        self.context = None;
        match index {
            0 => self.open_window(WindowKind::Terminal),
            1 => {
                if self.find(WindowKind::Files).is_none() { self.files.refresh(); }
                self.open_window(WindowKind::Files);
            }
            2 => self.open_window(WindowKind::Settings),
            3 => self.open_window(WindowKind::About),
            _ => {}
        }
        invalidate();
    }

    pub(super) fn draw_context(&self, f: &mut [u32], mx: i32, my: i32) {
        let Some((x,y,selected)) = self.context else { return; };
        rounded_rect(f,self.width,self.height,x,y,220,166,10,BORDER_ACTIVE);
        rounded_rect(f,self.width,self.height,x+1,y+1,218,164,9,WINDOW_BG);
        for (i,label) in ITEMS.iter().enumerate() {
            let r = Rect::new(x+6,y+8+i as i32*30,208,28);
            if r.contains(mx,my) || selected == i {
                rounded_rect(f,self.width,self.height,r.x,r.y,r.w,r.h,5,
                    if mouse::buttons() & mouse::BUTTON_LEFT != 0 { MENU_TILE_SELECTED } else { MENU_TILE_HOVER });
            }
            draw_text(f,self.width,self.height,x+14,r.y+4,x+212,label,FG);
        }
    }
}

pub fn key(event: KeyEvent) -> bool {
    let mut wm = WM.lock();
    let Some((x,y,i)) = wm.context else { return false; };
    match event.key {
        Key::Escape => wm.context = None,
        Key::Up => wm.context = Some((x,y,(i+ITEMS.len()-1)%ITEMS.len())),
        Key::Down => wm.context = Some((x,y,(i+1)%ITEMS.len())),
        Key::Enter => wm.context_action(i),
        _ => {}
    }
    invalidate();
    true
}
