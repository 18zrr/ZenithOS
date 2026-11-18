//! The Zenith spark mark, embedded as a 128x128 RGB bitmap.
//!
//! The artwork has a black backdrop, which is convenient: the compositor uses
//! a screen blend, so the black square disappears into any dark surface while
//! the violet glow stays intact. Bilinear sampling keeps small taskbar or menu
//! renditions smooth.

use crate::font::blend;

const SIZE: usize = 128;
static PIXELS: &[u8] = include_bytes!("logo_data.rgb");

#[inline]
fn sample(x: usize, y: usize) -> (u32, u32, u32) {
    let offset = (y * SIZE + x) * 3;
    (
        PIXELS[offset] as u32,
        PIXELS[offset + 1] as u32,
        PIXELS[offset + 2] as u32,
    )
}

#[inline]
fn lerp(a: u32, b: u32, fraction: u32) -> u32 {
    (a * (256 - fraction) + b * fraction) >> 8
}

#[inline]
fn screen_channel(destination: u32, source: u32) -> u32 {
    255 - ((255 - destination) * (255 - source)) / 255
}

/// Bilinear sample of the embedded bitmap, exposed for the boot splash which
/// paints into the linear framebuffer instead of a compositor surface.
pub fn bilinear_rgb(x0: usize, y0: usize, x1: usize, y1: usize, fx: u32, fy: u32) -> (u32, u32, u32) {
    let (r00, g00, b00) = sample(x0, y0);
    let (r10, g10, b10) = sample(x1, y0);
    let (r01, g01, b01) = sample(x0, y1);
    let (r11, g11, b11) = sample(x1, y1);
    (
        lerp(lerp(r00, r10, fx), lerp(r01, r11, fx), fy),
        lerp(lerp(g00, g10, fx), lerp(g01, g11, fx), fy),
        lerp(lerp(b00, b10, fx), lerp(b01, b11, fx), fy),
    )
}

/// Draw the mark into a compositor frame with a screen blend.
pub fn draw(
    frame: &mut [u32],
    stride: usize,
    height: usize,
    x: i32,
    y: i32,
    size: usize,
) {
    draw_with_opacity(frame, stride, height, x, y, size, 255);
}

/// Draw the mark as a dimmed watermark. `opacity` scales how much of the
/// screen-blended result reaches the destination.
pub fn draw_dim(
    frame: &mut [u32],
    stride: usize,
    height: usize,
    x: i32,
    y: i32,
    size: usize,
    opacity: u8,
) {
    draw_with_opacity(frame, stride, height, x, y, size, opacity);
}

fn draw_with_opacity(
    frame: &mut [u32],
    stride: usize,
    height: usize,
    x: i32,
    y: i32,
    size: usize,
    opacity: u8,
) {
    if size == 0 || opacity == 0 {
        return;
    }
    let scale = ((SIZE as u64) << 16) / size as u64;
    for dy in 0..size {
        let py = y + dy as i32;
        if py < 0 || py as usize >= height {
            continue;
        }
        let sample_y = (dy as u64 * scale + scale / 2).saturating_sub(1 << 15);
        let y0 = ((sample_y >> 16) as usize).min(SIZE - 1);
        let y1 = (y0 + 1).min(SIZE - 1);
        let fy = ((sample_y >> 8) & 0xFF) as u32;

        for dx in 0..size {
            let px = x + dx as i32;
            if px < 0 || px as usize >= stride {
                continue;
            }
            let sample_x = (dx as u64 * scale + scale / 2).saturating_sub(1 << 15);
            let x0 = ((sample_x >> 16) as usize).min(SIZE - 1);
            let x1 = (x0 + 1).min(SIZE - 1);
            let fx = ((sample_x >> 8) & 0xFF) as u32;

            let (r00, g00, b00) = sample(x0, y0);
            let (r10, g10, b10) = sample(x1, y0);
            let (r01, g01, b01) = sample(x0, y1);
            let (r11, g11, b11) = sample(x1, y1);
            let sr = lerp(lerp(r00, r10, fx), lerp(r01, r11, fx), fy);
            let sg = lerp(lerp(g00, g10, fx), lerp(g01, g11, fx), fy);
            let sb = lerp(lerp(b00, b10, fx), lerp(b01, b11, fx), fy);

            let index = py as usize * stride + px as usize;
            let destination = frame[index];
            let dr = (destination >> 16) & 0xFF;
            let dg = (destination >> 8) & 0xFF;
            let db = destination & 0xFF;
            let blended = (screen_channel(dr, sr) << 16)
                | (screen_channel(dg, sg) << 8)
                | screen_channel(db, sb);
            frame[index] = if opacity == 255 {
                blended
            } else {
                blend(destination, blended, opacity)
            };
        }
    }
}

/// Draw the mark centred horizontally on `center_x`.
pub fn draw_centered(
    frame: &mut [u32],
    stride: usize,
    height: usize,
    center_x: i32,
    y: i32,
    size: usize,
) {
    draw(frame, stride, height, center_x - size as i32 / 2, y, size);
}

/// Blend a soft violet halo behind the mark. Used by the splash screen.
pub fn halo(
    frame: &mut [u32],
    stride: usize,
    height: usize,
    center_x: i32,
    center_y: i32,
    radius: i32,
    strength: u8,
) {
    if radius <= 0 {
        return;
    }
    let radius_squared = radius as i64 * radius as i64;
    for y in (center_y - radius).max(0)..(center_y + radius).min(height as i32) {
        let dy = (y - center_y) as i64;
        for x in (center_x - radius).max(0)..(center_x + radius).min(stride as i32) {
            let dx = (x - center_x) as i64;
            let distance = dx * dx + dy * dy;
            if distance >= radius_squared {
                continue;
            }
            let falloff = ((radius_squared - distance) * strength as i64 / radius_squared) as u8;
            let index = y as usize * stride + x as usize;
            frame[index] = blend(frame[index], 0x0055_3CD8, falloff);
        }
    }
}
