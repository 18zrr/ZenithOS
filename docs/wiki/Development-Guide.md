# Development Guide

## Conventions

- Language: Rust, edition 2021; assembly for boot entry points.
- Kernel, filesystem, GUI, and application libraries are `no_std` and use
  `extern crate alloc`.
- Every public function and struct should have documentation comments; complex
  logic should be explained in code comments.
- Architectural decisions belong in `DECISIONS.md`; keep this wiki and
  `RELEASE_NOTES.md` in sync when behaviour changes.
- Keep components modular and loosely coupled through small traits
  (`BlockDevice`, `FileSystem`, `Driver`).
- Minimal dependencies; prefer verifying behaviour in code and tests over
  assuming hardware behaviour or memory layout.

## Workflow

1. `cargo build --workspace` and `cargo test --workspace` before and after a
   change.
2. Verify boot in QEMU with `cd kernel && make qemu` (PVH), and, when boot/image
   code changed, rebuild and boot the BIOS, UEFI, and ISO images.
3. Test on real hardware only after reliable virtual testing.
4. Never modify host system files without explicit approval.
5. Record notable changes in `RELEASE_NOTES.md` and the relevant wiki page.

## Verification baseline

```
cargo build --workspace   → clean build
cargo test --workspace    → 26 tests / 40 suites / 0 failures
Boot paths: PVH direct load (text), Limine BIOS image, Limine UEFI image, bootable ISO
```

## Source layout

| Area | Path |
|---|---|
| Kernel | `kernel/src/` |
| Boot configurations | `boot/` |
| Drivers | `drivers/` and `kernel/src/{pci,ata,acpi,e1000,net,usb,nvidia,mouse,keyboard}.rs` |
| Filesystems | `filesystem/zenith-fs/` |
| GUI model | `gui/zenith-ui/` (host `Surface`/theme; the live desktop is `kernel/src/wm.rs`) |
| Applications | `userland/` |
| Tooling | `tools/` |

## Rules of thumb

- Describe only implemented, verified behaviour in documentation.
- Do not claim unfinished features work (RX, USB enumeration, APIC preemption,
  general paging, VFS mount table, syscall-driven userland, native GPU driver).
- Ask before major architectural changes; do not remove existing functionality
  without approval.

Related: [Architecture](Architecture.md), [Build System](Build-System.md),
[Roadmap](Roadmap.md).
