//! UI coverage masks; the shell and recovery text retain their fixed-cell grid.
//! DejaVu license: LICENSES/DejaVu.txt. Regenerate with tools/generate_ui_art.py.
static FONT: &[u8; 95 * 361] = include_bytes!("ui_font.alpha");
pub static ICONS: &[u8; 6 * 32 * 32] = include_bytes!("app_icons.alpha");

fn glyph(c: u8) -> &'static [u8] {
    let c = if (32..127).contains(&c) { c } else { b'?' };
    let start = usize::from(c - 32) * 361;
    &FONT[start..start + 361]
}

pub fn text_width(text: &str) -> i32 {
    text.bytes().map(|c| i32::from(glyph(c)[0])).sum()
}

pub fn text(mut x: i32, y: i32, right: i32, text: &str, mut pixel: impl FnMut(i32, i32, u8)) {
    for c in text.bytes() {
        let mask = glyph(c);
        let advance = i32::from(mask[0]);
        if x + advance > right { break; }
        for (dy, row) in mask[1..].chunks_exact(18).enumerate() {
            for (dx, &a) in row.iter().enumerate() {
                let px = x + dx as i32 - 1;
                if a != 0 && px < right { pixel(px, y + dy as i32, a); }
            }
        }
        x += advance;
    }
}
