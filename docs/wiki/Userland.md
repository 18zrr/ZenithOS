# Userland

## Applications

Seven `no_std` application libraries live under `userland/`:

| Directory | Application |
|---|---|
| `userland/terminal/` | Terminal |
| `userland/filemanager/` | File manager |
| `userland/systemsettings/` | System settings |
| `userland/calculator/` | Calculator |
| `userland/browser/` | Browser |
| `userland/mediaplayer/` | Media player |
| `userland/systemmonitor/` | System monitor |

Each crate uses `extern crate alloc` and has a host demo binary. Build them all
with `./build.sh`.

## What is real today

- The libraries compile and their meaningful logic is unit-tested.
- The **terminal** has real command parsing (not just a demo stub).
- `gui/zenith-ui/` now provides a real `Surface` software render target with
  `clear`/`fill_rect`/`get`, a shared Fluent-style dark `Theme`, and a
  `ZenithUIManager` that renders a wallpaper, window and taskbar into that
  surface (unit-tested). The kernel's framebuffer driver is separate, so the
  crate's surface is not yet wired to the screen — it remains a model and test
  target rather than the live compositor.

## Important distinction

These applications run as **host harnesses**, not as OS processes. Zenith OS does
not yet have a syscall-driven userland: the kernel's `syscalls.rs` installs the
`STAR`/`LSTAR`/`FMASK` MSRs and a small read/write/open/close dispatcher, but no
user process uses it. There is no login; the kernel runs as root.

The interactive desktop and its windows are part of the kernel, not these
crates: `kernel/src/wm.rs` draws the compositor, window manager, taskbar, Start
menu and built-in windows (Terminal, System Monitor, Settings, About), and
`kernel/src/task.rs` runs the shell loop and routes mouse and keyboard events.
The interactive shell is likewise kernel code. See [Scheduling](Scheduling.md)
and [Graphics](Graphics.md).

## Planned work

Running apps as syscall-driven processes and wiring Zenith UI rendering to the
framebuffer are future goals — see [Roadmap](Roadmap.md).
