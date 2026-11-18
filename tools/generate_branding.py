#!/usr/bin/env python3
"""Derive Zenith OS branding assets from the master artwork.

The source is the supplied four-pointed spark: assets/branding/zenith-logo-source.png.
Everything else is generated from it so the kernel, boot splash and Limine
wallpaper always use the same mark.

Outputs:
  assets/branding/zenith-logo.png   1024x1024 master crop
  assets/branding/zenith-logo-512.png
  kernel/src/logo_data.rgb          128x128 RGB bytes embedded in the kernel
  boot/wallpaper.png                1920x1080 Limine wallpaper

The artwork sits on pure black. The kernel compositor and the Limine wallpaper
both screen-blend it, so black simply disappears and the violet glow survives.
"""

from __future__ import annotations

from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
ASSETS = ROOT / "assets" / "branding"
BOOT = ROOT / "boot"
KERNEL = ROOT / "kernel" / "src"

SOURCE = ASSETS / "zenith-logo-source.png"
FALLBACK = Path.home() / "Pictures" / "ZenithOS logo.png"

# Square crop around the mark, in source pixels. The measured ink bounds are
# x 315..958, y 314..894; this keeps a little breathing room for the glow.
CROP = (276, 244, 996, 964)


def load_master() -> Image.Image:
    path = SOURCE if SOURCE.exists() else FALLBACK
    if not path.exists():
        raise SystemExit(f"branding source not found: {path}")
    image = Image.open(path).convert("RGB").crop(CROP)
    # The artwork's backdrop is not quite #000000, which would leave a visible
    # rectangle when screen-blended. Lift the black point so corners are pure
    # black while the violet glow survives.
    pixels = np.asarray(image, dtype=np.float32) / 255.0
    floor = 12.0 / 255.0
    pixels = np.clip((pixels - floor) / (1.0 - floor), 0.0, 1.0)
    return Image.fromarray((pixels * 255.0 + 0.5).astype(np.uint8))


def screen(base: np.ndarray, layer: np.ndarray) -> np.ndarray:
    return 1.0 - (1.0 - base) * (1.0 - layer)


def lerp(a: np.ndarray, b: np.ndarray, t: np.ndarray) -> np.ndarray:
    return a + (b - a) * t


def build_wallpaper(master: Image.Image) -> Image.Image:
    width, height = 1920, 1080
    yy, xx = np.mgrid[0:height, 0:width].astype(np.float32)
    vertical = (yy / height)[..., None]
    top = np.array([0.008, 0.010, 0.026], dtype=np.float32)
    bottom = np.array([0.026, 0.016, 0.058], dtype=np.float32)
    base = lerp(top, bottom, vertical)

    # Violet aurora behind the mark, strongest on the right.
    cx, cy = width * 0.70, height * 0.46
    distance = np.sqrt(
        ((xx - cx) / (width * 0.46)) ** 2 + ((yy - cy) / (height * 0.70)) ** 2
    )
    haze = np.clip(1.0 - distance, 0.0, 1.0) ** 2.6
    base = screen(base, np.array([0.10, 0.05, 0.28], dtype=np.float32) * haze[..., None])

    # A cool counter-glow on the left keeps the composition from going muddy.
    cx2, cy2 = width * 0.12, height * 0.72
    distance2 = np.sqrt(
        ((xx - cx2) / (width * 0.42)) ** 2 + ((yy - cy2) / (height * 0.55)) ** 2
    )
    haze2 = np.clip(1.0 - distance2, 0.0, 1.0) ** 2.8
    base = screen(base, np.array([0.03, 0.08, 0.20], dtype=np.float32) * haze2[..., None])

    # Faint vignette keeps the Limine menu text readable.
    vignette = 1.0 - 0.42 * np.clip(
        ((xx / width - 0.5) ** 2 + (yy / height - 0.5) ** 2) * 2.6, 0.0, 1.0
    )
    base *= vignette[..., None]

    canvas = Image.fromarray((np.clip(base, 0, 1) * 255).astype(np.uint8))

    size = 720
    mark = master.resize((size, size), Image.LANCZOS)
    plate = Image.new("RGB", canvas.size, (0, 0, 0))
    plate.paste(mark, (width - size - 190, (height - size) // 2 - 10))

    base_np = np.asarray(canvas, dtype=np.float32) / 255.0
    mark_np = np.asarray(plate, dtype=np.float32) / 255.0
    merged = screen(base_np, mark_np)
    return Image.fromarray((np.clip(merged, 0, 1) * 255).astype(np.uint8))


def main() -> None:
    ASSETS.mkdir(parents=True, exist_ok=True)
    BOOT.mkdir(parents=True, exist_ok=True)

    master = load_master()
    master = master.resize((1024, 1024), Image.LANCZOS)
    master.save(ASSETS / "zenith-logo.png", optimize=True)
    master.resize((512, 512), Image.LANCZOS).save(
        ASSETS / "zenith-logo-512.png", optimize=True
    )

    # The kernel samples this with a screen blend, so black is transparent.
    kernel_bitmap = master.resize((128, 128), Image.LANCZOS)
    (KERNEL / "logo_data.rgb").write_bytes(kernel_bitmap.tobytes())

    wallpaper = build_wallpaper(master)
    wallpaper.save(BOOT / "wallpaper.png", optimize=True)

    print(f"logo:      {ASSETS / 'zenith-logo.png'}")
    print(f"kernel:    {KERNEL / 'logo_data.rgb'} ({128 * 128 * 3} bytes)")
    print(f"wallpaper: {BOOT / 'wallpaper.png'}")


if __name__ == "__main__":
    main()
