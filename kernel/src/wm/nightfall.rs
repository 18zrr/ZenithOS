//! Nightfall: a cached alpine lake, drawn at the actual framebuffer resolution.
use super::{blend, glow, mix};

fn hash(mut n: u32) -> u32 {
    n ^= n >> 16;
    n = n.wrapping_mul(0x7feb352d);
    n ^= n >> 15;
    n.wrapping_mul(0x846ca68b) ^ (n >> 16)
}

// Piecewise-linear noise gives each ridge a chiseled silhouette.
fn ridge(x: usize, w: usize, count: usize, seed: u32) -> i32 {
    let phase = x * count * 256 / w;
    let k = (phase >> 8) as u32;
    let t = (phase & 255) as i32;
    let point = |i| ((hash(seed + i) >> 24) as i32) - 128;
    (point(k) * (256 - t) + point(k + 1) * t) / 256
}

pub(super) fn draw(f: &mut [u32], w: usize, h: usize) {
    let shore = h * 73 / 100;
    let (mx, my) = (w * 79 / 100, h * 24 / 100);
    let radius = (h / 21).max(2);
    glow(
        f,
        w,
        h,
        mx as i64,
        my as i64,
        (radius * 5) as i64,
        0xd3adcb,
        24,
    );
    for i in 0..(w * h / 4400) as u32 {
        let x = hash(2 * i + 1) as usize % w;
        let y = hash(2 * i + 2) as usize % shore.max(1);
        let p = y * w + x;
        f[p] = blend(f[p], 0xeee8f5, 65 + (hash(i) >> 25) as u8);
    }
    for y in my.saturating_sub(radius)..(my + radius + 1).min(h) {
        let dy = y as i32 - my as i32;
        let color = mix(0xfff4d5, 0xf4bcaf, y + radius - my, 2 * radius);
        for x in mx.saturating_sub(radius)..(mx + radius + 1).min(w) {
            let dx = x as i32 - mx as i32;
            let edge = radius.pow(2) as i32 - dx * dx - dy * dy;
            if edge > 0 {
                let p = y * w + x;
                f[p] = blend(
                    f[p],
                    color,
                    (edge * 255 / (2 * radius) as i32).min(255) as u8,
                );
            }
        }
    }
    for (layer, (base, amp, light, dark)) in [
        (54, 15, 0xc697aa, 0x877294),
        (61, 13, 0x907caa, 0x575d86),
        (66, 10, 0x626a91, 0x394864),
        (71, 5, 0x3a5065, 0x223749),
    ]
    .into_iter()
    .enumerate()
    {
        let seed = 491 + layer as u32 * 141;
        for x in 0..w {
            let coarse = ridge(x, w, 9 + layer, seed);
            let fine = ridge(x, w, 33, seed + 32);
            let elevation = coarse + fine / 3 + ridge(x, w, 101, seed + 80) / 10;
            let top = ((base * h as i32 / 100) + (elevation * amp * h as i32 / 12800))
                .clamp(0, shore as i32) as usize;
            let facet = ((fine + 128) / 2 + (coarse + 128) / 4).clamp(0, 255) as u8;
            let face = blend(light, dark, facet);
            for y in top..shore {
                f[y * w + x] = blend(
                    face,
                    dark,
                    ((y - top) * 200 / (h / 6).max(1)).min(220) as u8,
                );
            }
        }
    }
    // Compress and perturb the reflected ridgelines, then add broken moonlight.
    for y in shore..h {
        let depth = y - shore;
        let sample = shore.saturating_sub(1 + depth * 2);
        let shift = (hash(y as u32) >> 24) as i32 - 128;
        let shift = shift * (1 + (depth * 12 / h) as i32) / 32;
        let water = mix(0x66658b, 0x192d48, depth, h - shore);
        let streak = hash(y as u32 + 71);
        let glint = mx as i32 + (((streak >> 24) as i32 - 128) * (radius + depth / 4) as i32 / 128);
        for x in 0..w {
            let sx = (x as i32 + shift).clamp(0, w as i32 - 1) as usize;
            let mut c = blend(f[sample * w + sx], water, 165);
            let dist = (x as i32 - glint).unsigned_abs() as usize;
            let span = 1 + (streak as usize % radius) / 2;
            if streak & 3 == 0 && dist < span {
                c = blend(c, 0xffcfb8, ((span - dist) * 110 / span) as u8);
            }
            f[y * w + x] = c;
        }
    }
}
