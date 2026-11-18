#!/usr/bin/env python3
"""Generate kernel/src/font.rs from DejaVu Sans Mono.

The kernel uses a fixed 10x20 cell: 16 px DejaVu Sans Mono glyphs with
antialiased coverage in the alpha channel. Keeping one monospaced cell means
the terminal grid and all existing UI text positions stay predictable.
"""

from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


CELL_W = 10
CELL_H = 20
FONT_SIZE = 16
FIRST = 0
LAST = 127


def font_path() -> str:
    candidates = [
        "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    ]
    for candidate in candidates:
        if Path(candidate).exists():
            return candidate
    raise SystemExit("DejaVuSansMono.ttf not found; install fonts-dejavu-core")


def render(font: ImageFont.FreeTypeFont, code: int) -> list[list[int]]:
    if code < 32 or code == 127:
        return [[0] * CELL_W for _ in range(CELL_H)]
    image = Image.new("L", (CELL_W, CELL_H), 0)
    draw = ImageDraw.Draw(image)
    draw.text((0, 0), chr(code), font=font, fill=255)
    pixels = image.load()
    return [[pixels[x, y] for x in range(CELL_W)] for y in range(CELL_H)]


def emit(glyphs: list[list[list[int]]]) -> str:
    lines = [
        "//! Antialiased 10x20 monospaced glyphs for the framebuffer console and UI.",
        "//!",
        "//! Generated from DejaVu Sans Mono 16 px by tools/generate_font.py.",
        "//! DejaVu changes are in the public domain. The Bitstream Vera license",
        "//! terms are reproduced in LICENSES/DejaVu.txt.",
        "",
        "/// Glyph cell width in pixels.",
        "pub const GLYPH_WIDTH: usize = 10;",
        "/// Glyph cell height in pixels.",
        "pub const GLYPH_HEIGHT: usize = 20;",
        "/// Horizontal advance between text cells.",
        "pub const COL_PITCH: usize = 10;",
        "/// Vertical advance between text rows.",
        "pub const ROW_PITCH: usize = 20;",
        "",
        "/// Blend `color` over `dst` using an 8-bit coverage value.",
        "#[inline]",
        "pub fn blend(dst: u32, color: u32, alpha: u8) -> u32 {",
        "    let a = alpha as u32;",
        "    if a == 0 {",
        "        return dst;",
        "    }",
        "    if a == 255 {",
        "        return color;",
        "    }",
        "    let inv = 255 - a;",
        "    let rb = (((color & 0x00FF_00FF) * a + (dst & 0x00FF_00FF) * inv) >> 8)",
        "        & 0x00FF_00FF;",
        "    let g = (((color & 0x0000_FF00) * a + (dst & 0x0000_FF00) * inv) >> 8)",
        "        & 0x0000_FF00;",
        "    rb | g",
        "}",
        "",
        "/// Alpha coverage for ASCII 0-127. Zero is transparent, 255 is opaque.",
        "pub static FONT: [[[u8; GLYPH_WIDTH]; GLYPH_HEIGHT]; 128] = [",
    ]
    for code, glyph in enumerate(glyphs):
        label = "blank" if code < 32 or code == 127 else repr(chr(code))
        lines.append(f"    // 0x{code:02x} {label}")
        if code < 32 or code == 127:
            lines.append("    [[0; GLYPH_WIDTH]; GLYPH_HEIGHT],")
            continue
        lines.append("    [")
        for row in glyph:
            lines.append("        [" + ", ".join(f"0x{value:02x}" for value in row) + "],")
        lines.append("    ],")
    lines.append("];")
    lines.append("")
    return "\n".join(lines)


def main() -> None:
    font = ImageFont.truetype(font_path(), FONT_SIZE)
    glyphs = [render(font, code) for code in range(FIRST, LAST + 1)]
    out = Path(__file__).resolve().parent.parent / "kernel" / "src" / "font.rs"
    out.write_text(emit(glyphs))
    print(f"wrote {out} ({out.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
