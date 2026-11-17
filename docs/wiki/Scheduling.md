# Scheduling

## Model

`kernel/src/task.rs` implements a cooperative scheduler with a fixed array of
per-task kernel stacks. `switch_context` is an assembly routine that saves and
restores callee-saved registers and the stack pointer.

## Tasks

- **Shell task**: interactive echo over the PS/2 keyboard; Enter starts a new
  prompt line, Backspace deletes, F12 cycles framebuffer orientation.
- **Heartbeat task**: prints a tick marker roughly once per second.
- **Idle task**: yields forever.

## Yielding

Tasks call `yield_now()`, which selects the next runnable task and switches to
it. Interrupts are enabled during normal task execution.

## Current limitation

There is no APIC timer and no preemptive scheduling. A task that never yields
would monopolise the CPU. This is a planned improvement.

See [Kernel](Kernel.md) and [Interrupts](Interrupts.md).
