# Zenith OS Kernel

This directory contains the core kernel implementation for Zenith OS.

## Modules Implemented

### Memory Management (`memory.rs`)
- Physical and virtual address types
- Memory region structure
- Simple heap allocator (placeholder)
- Initialization functions

### Paging (`paging.rs`) 
- Page table entry structure
- Page table and directory structures
- Virtual-to-physical mapping capabilities
- Identity mapping setup

### Diagnostics (`diagnostics.rs`)
- Basic output functions for kernel messages
- Diagnostic initialization

### Interrupt Handling (`interrupt.rs`)
- Interrupt controller management (placeholder)

### Scheduler (`scheduler.rs`)
- Basic scheduling framework (placeholder)
- Task execution mechanisms 

### Process Management (`process.rs`)
- Process creation and management (placeholder)
- Process state tracking

### Synchronization Primitives (`sync.rs`)
- Spinlock implementation using spinning_top crate
- Basic synchronization primitives

## Build Instructions

To build the kernel:
```bash
make
```

To clean build artifacts:
```bash
make clean
```

To test compilation with no_std compatibility:
```bash
make test
```

To run clippy lints:
```bash  
make clippy
```

To format code:
```bash
make fmt
```

## Target Architecture

- Target: x86_64-unknown-none
- Build Profile: Release (optimized for size)
- Panic Strategy: Abort

## Dependencies

The kernel uses the following crates:
- `spinning_top` - For spinlock implementation compatible with no_std