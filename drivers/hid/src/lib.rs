//! Minimal HID report-descriptor parser for boot and non-boot mice.
//!
//! The kernel uses this to support report-protocol mice with more than three
//! buttons, a wheel, or horizontal pan. Parsing is deliberately limited to the
//! fields a pointer needs; keyboards continue to use the HID boot protocol.

#![cfg_attr(not(test), no_std)]

/// One positional or wheel field inside an input report.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Field {
    pub offset: u16,
    pub size: u8,
    pub signed: bool,
}

impl Field {
    pub fn present(&self) -> bool {
        self.size != 0
    }
}

/// Layout of a mouse input report.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct MouseLayout {
    pub report_id: u8,
    pub has_report_id: bool,
    pub buttons: u8,
    pub button_offset: u16,
    pub x: Field,
    pub y: Field,
    pub wheel: Field,
    pub pan: Field,
}

/// Decoded pointer report.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct MouseReport {
    pub buttons: u8,
    pub dx: i32,
    pub dy: i32,
    pub wheel: i32,
    pub pan: i32,
}

#[inline]
fn item_u32(data: &[u8], size: usize) -> u32 {
    let mut value = 0u32;
    for (index, byte) in data.iter().take(size).enumerate() {
        value |= (*byte as u32) << (index * 8);
    }
    value
}

/// Logical minimum is a signed item, so sign-extend it from its byte width.
#[inline]
fn item_i32(data: &[u8], size: usize) -> i32 {
    match size {
        1 => data.first().copied().unwrap_or(0) as i8 as i32,
        2 => u16::from_le_bytes([data[0], data[1]]) as i16 as i32,
        _ => item_u32(data, size) as i32,
    }
}

/// Parse a HID report descriptor and extract the first mouse pointer report.
pub fn parse_mouse(descriptor: &[u8]) -> Option<MouseLayout> {
    let mut layout = MouseLayout::default();
    let mut bit = 0u16;
    let mut usage_page = 0u16;
    let mut usages = [0u16; 32];
    let mut usage_count = 0usize;
    let mut usage_min = 0u16;
    let mut usage_range = false;
    let mut report_size = 0u8;
    let mut report_count = 0u16;
    let mut logical_min = 0i32;
    let mut saw_x = false;
    let mut saw_y = false;

    let mut index = 0usize;
    while index < descriptor.len() {
        let prefix = descriptor[index];
        index += 1;
        let size = match prefix & 0x03 {
            0 => 0,
            1 => 1,
            2 => 2,
            _ => 4,
        };
        // Long items are not used by pointer descriptors.
        if prefix == 0xFE {
            if index >= descriptor.len() {
                break;
            }
            let length = descriptor[index] as usize;
            index = index.saturating_add(1 + length);
            continue;
        }
        if index + size > descriptor.len() {
            break;
        }
        let data = &descriptor[index..index + size];
        index += size;
        let value = item_u32(data, size);

        match prefix & 0xFC {
            // Main items.
            0x80 => {
                for field_index in 0..report_count {
                    let usage = if usage_range {
                        usage_min.saturating_add(field_index)
                    } else if usage_count == 0 {
                        0
                    } else if (field_index as usize) < usage_count {
                        usages[field_index as usize]
                    } else {
                        usages[usage_count - 1]
                    };
                    if usage_page == 0x09 && (1..=5).contains(&usage) {
                        if layout.buttons == 0 {
                            layout.button_offset = bit;
                        }
                        layout.buttons = layout.buttons.saturating_add(1).min(5);
                    } else if usage_page == 0x01 || usage_page == 0x0c {
                        let field = Field {
                            offset: bit,
                            size: report_size,
                            signed: logical_min < 0,
                        };
                        match (usage_page, usage) {
                            (1, 0x30) if !layout.x.present() => {
                                layout.x = field;
                                saw_x = true;
                            }
                            (1, 0x31) if !layout.y.present() => {
                                layout.y = field;
                                saw_y = true;
                            }
                            (1, 0x38) if !layout.wheel.present() => layout.wheel = field,
                            (1, 0x36) | (0x0c, 0x238) if !layout.pan.present() => layout.pan = field,
                            _ => {}
                        }
                    }
                    bit = bit.saturating_add(report_size as u16);
                }
                usage_count = 0;
                usage_range = false;
            }
            0x90 | 0xB0 => {
                // Output and feature reports do not appear in input packets.
                usage_count = 0;
                usage_range = false;
            }
            0xC0 => {
                usage_count = 0;
                usage_range = false;
            }
            // Global items.
            0x04 => usage_page = value as u16,
            0x14 => logical_min = item_i32(data, size),
            0x24 => {}
            0x74 => report_size = value.min(32) as u8,
            0x84 => {
                // Only the first pointer report is decoded. A later report ID
                // belongs to a different report (for example consumer keys).
                if layout.has_report_id && (saw_x || saw_y) {
                    break;
                }
                let id = value as u8;
                layout.report_id = id;
                layout.has_report_id = id != 0;
                if layout.has_report_id {
                    bit = 0;
                }
            }
            0x94 => report_count = value.min(64) as u16,
            // Local items.
            0x08 => {
                if usage_count < usages.len() {
                    usages[usage_count] = (value & 0xFFFF) as u16;
                    usage_count += 1;
                }
                usage_range = false;
            }
            0x18 => {
                usage_min = (value & 0xFFFF) as u16;
                usage_range = true;
            }
            0x28 => {
                usage_range = true;
            }
            _ => {}
        }
    }

    if saw_x && saw_y {
        Some(layout)
    } else {
        None
    }
}

#[inline]
fn read_bits(report: &[u8], offset: u16, size: u8) -> u32 {
    let mut value = 0u32;
    for bit in 0..size as u16 {
        let position = usize::from(offset) + usize::from(bit);
        let byte = position / 8;
        if byte >= report.len() {
            break;
        }
        if report[byte] & (1 << (position % 8)) != 0 {
            value |= 1 << bit;
        }
    }
    value
}

#[inline]
fn signed(value: u32, size: u8) -> i32 {
    if size == 0 {
        return 0;
    }
    if size >= 32 {
        return value as i32;
    }
    let sign = 1u32 << (size - 1);
    if value & sign != 0 {
        (value | !((1u32 << size) - 1)) as i32
    } else {
        value as i32
    }
}

#[inline]
fn field_value(report: &[u8], field: Field) -> i32 {
    if !field.present() {
        return 0;
    }
    let raw = read_bits(report, field.offset, field.size);
    if field.signed {
        signed(raw, field.size)
    } else {
        raw as i32
    }
}

/// Decode an input report using a parsed mouse layout.
pub fn decode_mouse(layout: &MouseLayout, report: &[u8]) -> Option<MouseReport> {
    let body = if layout.has_report_id {
        if report.first().copied()? != layout.report_id {
            return None;
        }
        &report[1..]
    } else {
        report
    };
    let bits = body.len().saturating_mul(8);
    if [layout.x, layout.y, layout.wheel, layout.pan].iter().any(|field|
        field.present() && usize::from(field.offset) + usize::from(field.size) > bits)
        || usize::from(layout.button_offset) + usize::from(layout.buttons) > bits {
        return None;
    }

    let mut buttons = 0u8;
    for button in 0..layout.buttons.min(5) {
        let position = layout.button_offset + button as u16;
        let byte = position / 8;
        if byte as usize >= body.len() {
            break;
        }
        if body[byte as usize] & (1 << (position % 8)) != 0 {
            buttons |= 1 << button;
        }
    }

    Some(MouseReport {
        buttons,
        dx: field_value(body, layout.x),
        dy: field_value(body, layout.y),
        wheel: field_value(body, layout.wheel),
        pan: field_value(body, layout.pan),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Generic three-button mouse with an 8-bit wheel, as exposed by QEMU.
    const BASIC: &[u8] = &[
        0x05, 0x01, 0x09, 0x02, 0xA1, 0x01, 0x09, 0x01, 0xA1, 0x00, 0x05, 0x09, 0x19, 0x01, 0x29,
        0x03, 0x15, 0x00, 0x25, 0x01, 0x95, 0x03, 0x75, 0x01, 0x81, 0x02, 0x95, 0x01, 0x75, 0x05,
        0x81, 0x01, 0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x15, 0x81, 0x25, 0x7F, 0x75, 0x08, 0x95,
        0x02, 0x81, 0x06, 0x09, 0x38, 0x15, 0x81, 0x25, 0x7F, 0x75, 0x08, 0x95, 0x01, 0x81, 0x06,
        0xC0, 0xC0,
    ];

    // Five buttons, 12-bit signed X/Y, wheel and pan, behind report ID 2.
    const FULL: &[u8] = &[
        0x05, 0x01, 0x09, 0x02, 0xA1, 0x01, 0x85, 0x02, 0x05, 0x09, 0x19, 0x01, 0x29, 0x05, 0x15,
        0x00, 0x25, 0x01, 0x95, 0x05, 0x75, 0x01, 0x81, 0x02, 0x95, 0x01, 0x75, 0x03, 0x81, 0x01,
        0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x16, 0x00, 0xF8, 0x26, 0xFF, 0x07, 0x75, 0x0C, 0x95,
        0x02, 0x81, 0x06, 0x09, 0x38, 0x15, 0x81, 0x25, 0x7F, 0x75, 0x08, 0x95, 0x01, 0x81, 0x06,
        0x09, 0x36, 0x81, 0x06, 0xC0,
    ];

    #[test]
    fn parses_qemu_style_mouse() {
        let layout = parse_mouse(BASIC).expect("layout");
        assert_eq!(layout.buttons, 3);
        assert_eq!(layout.button_offset, 0);
        assert_eq!(layout.x.offset, 8);
        assert_eq!(layout.y.offset, 16);
        assert_eq!(layout.wheel.offset, 24);
        assert_eq!(layout.x.size, 8);
        assert!(layout.x.signed);
        assert!(!layout.has_report_id);
    }

    #[test]
    fn parses_five_button_report_id_mouse() {
        let layout = parse_mouse(FULL).expect("layout");
        assert!(layout.has_report_id);
        assert_eq!(layout.report_id, 2);
        assert_eq!(layout.buttons, 5);
        assert_eq!(layout.x.size, 12);
        assert_eq!(layout.y.offset, 20);
        assert_eq!(layout.wheel.offset, 32);
        assert_eq!(layout.pan.offset, 40);
    }

    #[test]
    fn decodes_buttons_wheel_and_pan() {
        let layout = parse_mouse(FULL).expect("layout");
        let report = [0x02, 0x1B, 0xFF, 0x0F, 0x00, 0xFF, 0x01];
        let decoded = decode_mouse(&layout, &report).expect("report");
        assert_eq!(decoded.buttons, 0b11011);
        assert_eq!(decoded.dx, -1);
        assert_eq!(decoded.dy, 0);
        assert_eq!(decoded.wheel, -1);
        assert_eq!(decoded.pan, 1);
    }

    #[test]
    fn rejects_wrong_report_id() {
        let layout = parse_mouse(FULL).expect("layout");
        assert!(decode_mouse(&layout, &[0x01, 0, 0, 0, 0, 0]).is_none());
    }

    #[test]
    fn decodes_consumer_horizontal_pan_after_a_feature_report() {
        let mut descriptor = BASIC[..BASIC.len() - 2].to_vec();
        descriptor.extend_from_slice(&[0xb1, 0x02, 0x05, 0x0c, 0x0a, 0x38, 0x02,
                                       0x81, 0x06, 0xc0, 0xc0]);
        let layout = parse_mouse(&descriptor).unwrap();
        assert_eq!(layout.pan.offset, 32);
        let decoded = decode_mouse(&layout, &[0, 0, 0, 1, 0xff]).unwrap();
        assert_eq!((decoded.wheel, decoded.pan), (1, -1));
        assert!(decode_mouse(&layout, &[0, 0, 0]).is_none());
    }

    #[test]
    fn rejects_descriptor_without_axes() {
        assert!(parse_mouse(&[0x05, 0x09, 0x19, 0x01, 0x29, 0x03, 0x95, 0x03, 0x75, 0x01, 0x81, 0x02]).is_none());
    }
}
