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
- `gui/zenith-ui/` is a UI **data model**, not a renderer.

## Important distinction

These applications run as **host harnesses**, not as OS processes. Zenith OS does
not yet have a syscall-driven userland: the kernel's `syscalls.rs` installs the
`STAR`/`LSTAR`/`FMASK` MSRs and a small read/write/open/close dispatcher, but no
user process uses it. There is no login; the kernel runs as root.

The interactive shell you see at boot is part of the kernel (`task.rs`), not one
of these userland apps. See [Scheduling](Scheduling.md).

## Planned work

Running apps as syscall-driven processes and wiring Zenith UI rendering to the
framebuffer are future goals — see [Roadmap](Roadmap.md).
