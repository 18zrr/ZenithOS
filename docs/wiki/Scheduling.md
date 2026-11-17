# Scheduling

## Model

`kernel/src/task.rs` implements a cooperative scheduler with a fixed array of
per-task kernel stacks. `switch_context` is an assembly routine that saves and
restores callee-saved registers and the stack pointer.

## Tasks

- **Shell task**: interactive line editor over the PS/2 keyboard with commands
  `help`, `sysinfo`, `pci`, `mem`, `net`, `gpu`, `acpi`, `history`, `version`,
  `about`, `echo`, `uptime`, `clear`, and `flip`, all backed by real subsystem
  data. Backspace edits the line, Up/Down recall history, Enter runs it, and
  F12 cycles framebuffer orientation.
- **Heartbeat task**: polls the NIC and writes a tick marker roughly once per
  second to COM1 serial only (not to the on-screen console).
- **Idle task**: yields forever.

## Yielding

Tasks call `yield_now()`, which selects the next runnable task and switches to
it. Interrupts are enabled during normal task execution.

## Current limitation

There is no APIC timer and no preemptive scheduling. A task that never yields
would monopolise the CPU. This is a planned improvement.

See [Kernel](Kernel.md) and [Interrupts](Interrupts.md).
