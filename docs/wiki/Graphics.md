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

The desktop is a Windows 11-inspired compositing environment implemented in
`kernel/src/wm.rs`.

- **Compositor**: a gradient wallpaper is generated once and cached. On each
  redraw the wallpaper, every visible window, the taskbar, the Start menu and
  the mouse cursor are composed into a full-screen RAM buffer (about 6 MiB at
  1024×768×32 bpp), which is then blitted to the framebuffer in one pass. This
  avoids the tearing and flicker of direct per-widget framebuffer drawing.
- **Dirty redraws**: a frame is only composed when something changed (input,
  focus, window state, menu state, caret blink or clock minute) and never more
  than roughly 50 times per second.
- **Windows**: Terminal, System Monitor, Settings and About. Each has a title
  bar with a per-app icon letter, minimise / maximise / close buttons, rounded
  corners, a drop shadow while floating, and a bottom-right resize grip.
  Clicking a window focuses and raises it; dragging the title bar moves it;
  dragging the grip resizes it within the screen bounds.
- **Taskbar**: a centred Start button and pinned app icons with
  running/focused indicators and hover feedback, a link-status indicator, and
  a real-time clock read from CMOS. The clock asks for a repaint when the
  minute changes, so it advances without input.
- **Start menu**: click Start or press **F1**. It has a search field, app
  tiles, keyboard navigation (arrows, Enter, Esc) and mouse selection. Typing
  filters the list; clicking outside the panel closes it.
- **Settings**: four accent swatches change the accent colour live — window
  accents, Start button, taskbar indicators, headings and the terminal caret.

The verbose boot log is suppressed on screen: `boot_complete()` switches to
the compositor only after all subsystems are ready, so the screen opens on a
clean desktop instead of debug spam. Full logs still go to COM1 serial.

Two other full-screen surfaces are drawn: the boot splash during
initialisation (see [Boot Process](Boot-Process.md)) and the five-step
installation wizard opened from the Start menu or with `install`. The wizard
is modal and draws directly to the framebuffer; mouse events are flushed
around it and the desktop is restored when it exits.

## Mouse

- PS/2 mouse on IRQ12 (`kernel/src/mouse.rs`), three-byte packet assembly with
  the always-set bit validated, sign-extended X/Y deltas, and left, right and
  middle buttons.
- The cursor is clamped to the screen and drawn by the compositor.
- Movement and button events are queued (128 entries) and drained by the shell
  loop, so the interrupt handler never draws.
- USB HID mice are **not** supported; USB enumeration currently times out. See
  [Drivers](Drivers.md).

## Text console

`kernel/src/framebuffer.rs` implements the console drawn inside the Terminal
window:

- 8×8 bitmap font from `font.rs`, scaled 2× into 16×16 px glyphs, with a 16 px
  column pitch and 20 px row pitch.
- A shadow text buffer of up to 100 columns × 40 rows. The visible console size
  is recomputed from the Terminal window's content area (56×20 at the default
  window size), so resizing the window changes the number of columns and rows.
- Each cell is cleared with a background colour before its glyph is drawn, so
  characters do not ghost or overlap.
- When the cursor passes the last visible row, the shadow buffer scrolls up by
  one row and the newly exposed row is cleared.
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
  registers (no GSP firmware, engine programming, or mode setting). All
  compositing is done on the CPU.
- Fixed 1024×768 software compositor with no HiDPI scaling, multi-monitor
  support, window snapping, Alt-Tab switcher, or animations.
- Mouse input is PS/2 only; USB HID is not enumerated.
- The graphical desktop needs a Limine-provided linear framebuffer. QEMU's
  direct PVH/multiboot load has none and stays on the text shell.
