"""Build the compact coverage masks used by Zenith's proportional UI and icons."""
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

OUT = Path(__file__).resolve().parents[1] / "kernel/src"


def typeface():
    face = ImageFont.truetype("DejaVuSans.ttf", 16)
    data = bytearray()
    for code in range(32, 127):
        ch = chr(code)
        glyph = Image.new("L", (18, 20))
        ImageDraw.Draw(glyph).text((1, 0), ch, font=face, fill=255)
        data.append(round(face.getlength(ch)))
        data.extend(glyph.tobytes())
    (OUT / "ui_font.alpha").write_bytes(data)


def icons():
    # 32-unit viewbox, supersampled for legibility even in a title bar.
    data = bytearray()
    for name in ("terminal", "monitor", "settings", "about", "installer", "browser"):
        image = Image.new("L", (128, 128))
        d = ImageDraw.Draw(image)

        def line(*points):
            d.line([(x*4, y*4) for x, y in points], fill=255, width=8, joint="curve")

        def oval(x, y, r, ry=None):
            ry = ry if ry is not None else r
            d.ellipse(((x-r)*4, (y-ry)*4, (x+r)*4, (y+ry)*4), outline=255, width=8)

        if name == "terminal":
            line((7, 10), (13, 16), (7, 22))
            line((16, 22), (25, 22))
        elif name == "monitor":
            d.rounded_rectangle((20, 26, 108, 89), radius=8, outline=255, width=8)
            line((12, 26), (20, 26))
            line((8, 16), (12, 16), (15, 11), (18, 18), (21, 14), (24, 14))
        elif name == "settings":
            for x, y in [(8, 13), (16, 21), (24, 11)]:
                line((x, 6), (x, y-3)); line((x, y+3), (x, 26))
                oval(x, y, 3)
        elif name == "about":
            oval(16, 16, 11)
            line((16, 15), (16, 22))
            d.ellipse((59, 38, 69, 48), fill=255)
        elif name == "installer":
            line((16, 5), (16, 19)); line((10, 14), (16, 20), (22, 14))
            line((6, 21), (6, 26), (26, 26), (26, 21))
        else:
            oval(16, 16, 11); oval(16, 16, 5, 11)
            line((5, 16), (27, 16))
        data.extend(image.resize((32, 32), Image.Resampling.LANCZOS).tobytes())
    (OUT / "app_icons.alpha").write_bytes(data)


if __name__ == "__main__":
    typeface()
    icons()
