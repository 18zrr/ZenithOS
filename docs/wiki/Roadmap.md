# Roadmap

This page lists planned work only. Items here are **not** claimed as working;
see [Status](Status.md) for the verified baseline.

## Near term

1. **e1000 RX and networking** — complete and validate the receive path;
   verify live DHCP/UDP/TCP exchange, not just TX.
2. **USB enumeration** — make UHCI control transfers complete, add a HID class
   driver, and support EHCI/xHCI beyond class detection.
3. **APIC preemption** — add an APIC timer and preemptive scheduling; the
   current scheduler is cooperative ([Scheduling](Scheduling.md)).
4. **Generalise paging** — a page allocator and recovery for arbitrary faults
   beyond the single test page ([Memory Management](Memory-Management.md)).

## Medium term

5. **VFS mount table** — unify the existing `FileSystem` trait into a real
   mount layer; currently mounting is manual ([Filesystems](Filesystems.md)).
6. **Syscall-driven userland** — run applications as OS processes using the
   syscall dispatcher instead of host harnesses ([Userland](Userland.md)).
7. **Zenith UI rendering** — wire the UI data model to the framebuffer and add
   a window manager; today only a static desktop is drawn ([Graphics](Graphics.md)).

## Longer term / out of scope for v0.1.x

8. **Native GPU mode setting** — requires proprietary GSP firmware and hardware
   not available in this environment; `nvidia.rs` currently only probes
   ([Drivers](Drivers.md)).
9. **Linux compatibility layer** — an OS-scale effort; not implemented.
10. **Pure cross-compiled kernel** — add `x86_64-unknown-none` once `rustup` is
    available; current images are built from a host static library.

## Guiding principle

Documentation must describe only implemented, verified features. Items are
moved from this page to [Status](Status.md) only after they are built, tested,
and boot-verified.
