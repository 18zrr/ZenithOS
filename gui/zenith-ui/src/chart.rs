//! Allocation-free rendering for bounded percentage histories on RGB surfaces.

/// Draw uniformly spaced, bottom-aligned samples (oldest first) into a
/// clipped rectangle. An empty history clears the chart. Callers can pass
/// only the populated suffix and reserve earlier columns with `capacity`.
pub fn percentage_chart(
    pixels: &mut [u32],
    stride: usize,
    bounds: (i32, i32, usize, usize),
    samples: &[u8],
    capacity: usize,
    foreground: u32,
    background: u32,
) {
    let (x, y, width, height) = bounds;
    if stride == 0 || width == 0 || height == 0 || capacity == 0 {
        return;
    }
    let samples = &samples[samples.len().saturating_sub(capacity)..];
    let rows = pixels.len() / stride;
    let left = (x as i64).clamp(0, stride as i64) as usize;
    let right = (x as i64)
        .saturating_add_unsigned(width as u64)
        .clamp(0, stride as i64) as usize;
    let top = (y as i64).clamp(0, rows as i64) as usize;
    let bottom = (y as i64)
        .saturating_add_unsigned(height as u64)
        .clamp(0, rows as i64) as usize;
    for px in left..right {
        let column = (px as i64 - i64::from(x)) as u128 * capacity as u128 / width as u128;
        let sample = (column as usize)
            .checked_sub(capacity - samples.len())
            .and_then(|i| samples.get(i));
        let filled = sample.map_or(0, |s| u128::from((*s).min(100)) * height as u128 / 100);
        for py in top..bottom {
            let row = (py as i64 - i64::from(y)) as u128;
            pixels[py * stride + px] = if row >= height as u128 - filled {
                foreground
            } else {
                background
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clips_without_rescaling_and_right_aligns_partial_history() {
        let mut pixels = [9; 12];
        percentage_chart(&mut pixels, 4, (-1, -1, 4, 3), &[50, 110], 4, 1, 0);
        assert_eq!(pixels, [0, 0, 1, 9, 0, 1, 1, 9, 9, 9, 9, 9]);
    }
}
