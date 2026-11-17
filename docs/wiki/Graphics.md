# Graphics

Zenith OS graphics come from a **Limine-provided linear framebuffer** (VBE on
BIOS, GOP on UEFI). There is no native GPU driver.

## Framebuffer

- The native Limine protocol requests a linear framebuffer tagged type
  `MB2_FRAMEBUFFER_TYPE_RGB`, parsed by `kernel/src/limine.rs` and
  `kernel/src/framebuffer.rs`.
- Verified mode: **1024×768×32 bpp**, with pitch and address taken from the
  bootloader tag.
- On the Limine path the kernel keeps Limine's page tables and HHDM rather than
  replacing CR3 (see [Memory Management](Memory-Management.md)).

## Desktop

`draw_desktop()` renders a minimal desktop into the framebuffer:

- solid background,
- a top bar,
- a simple window,
- a centred dock with seven coloured icons.

This is a static drawing, not a window manager. Zenith UI is currently a data
model only (see [Userland](Userland.md)), so no UI elements are wired to the
framebuffer beyond this desktop and text console.

## Text console

`kernel/src/framebuffer.rs` implements the on-screen console:

- 8×8 bitmap font from `font.rs`, scaled 2× into 16×16 px cells.
- An 80-column × 25-row text grid.
- Each cell is cleared with a background colour before its glyph is drawn, so
  characters do not ghost or overlap.
- When the cursor passes the last row, the frame scrolls up by one row and the
  newly exposed row is cleared.
- `diagnostics::puts` mirrors output to COM1 serial and to this console.

## Display orientation

Some VMs present the framebuffer rotated or mirrored. Orientation is handled in
one place (`mapped()`), so all drawing — pixels, glyphs, cell clearing, desktop,
and scrolling — stays coherent.

- **Runtime:** press **F12** in the VM. The first press applies a 180° rotation;
  further presses cycle through mirror-X, flip-Y, and normal. The active mode is
  printed to the console after each press. F12 scancode (`0x58`) is captured by a
  raw-scancode ring in `keyboard.rs` and handled in the shell loop in `task.rs`.
- **Permanent:** rebuild with `ZENITH_FB_FLIP=xy ./build_iso.sh`. Valid values
  are `x`, `y`, and `xy`; omit for the default orientation used by QEMU.

See [Troubleshooting](Troubleshooting.md) for display problems.

## Limitations

- No native GPU driver; `nvidia.rs` only detects devices and reads boot
  registers (no GSP firmware, engine programming, or mode setting).
- No compositing, window management, mouse cursor, or UI rendering.
