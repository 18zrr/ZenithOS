# Interrupts

## IDT

`kernel/src/interrupt.rs` installs 48 interrupt gates:

- vectors 0–31: CPU exceptions
- vectors 32–47: IRQs from the remapped 8259A PIC

## PIC and PIT

The master PIC is remapped to vectors 0x20–0x2F and the slave to 0x28–0x2F.
IRQ0 (timer) and IRQ1 (keyboard) are unmasked. The PIT channel 0 is programmed
for periodic mode 3 at 100 Hz with explicit EOI.

## Exception reporting

All 32 CPU exceptions have descriptive names. Unexpected exceptions print the
vector, error code, RIP, RFLAGS, and general-purpose registers before halting.

## Page faults

The page-fault handler:

1. reads CR2,
2. tries demand-page recovery for the test region,
3. otherwise decodes `P`, `W/R`, `U/S`, `RSVD`, and `I/D` bits,
4. prints RIP and halts.

## Keyboard and timer interaction

The 100 Hz tick drives the heartbeat task and the shell's periodic checks. The
keyboard IRQ stores translated characters in a buffer and raw scancodes in a
separate 16-entry ring used by the F12 orientation toggle.

See [Scheduling](Scheduling.md) and [Graphics](Graphics.md).
