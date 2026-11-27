# Zenith OS Architecture

## Overview
Zenith OS is a custom operating system built from the ground up with a monolithic kernel architecture. It's designed to be completely independent of existing Linux distributions or systems.

## Components

### Kernel
- **Language**: Rust (with inline assembly where needed)
- **Architecture**: x86_64
- **Type**: Monolithic kernel 
- **Memory Management**: Custom allocator with paging support
- **Scheduling**: Basic process scheduling capabilities

### Filesystem
- **Primary FS**: Zenith FS - a custom filesystem implementation  
- **Compatibility**: FAT32 and other common formats for compatibility

### Graphical Environment  
- **Name**: Zenith UI
- **Design**: Combination of macOS and Windows concepts with dark theme
- **Rendering**: Framebuffer graphics initially, Vulkan support planned

### Applications
- **Primary**: Custom applications written in Rust or C
- **Compatibility Layer**: Future support for Linux applications via Proton/Wine

## System Structure

```
zenith-os/
├── ARCHITECTURE.md         # This file  
├── ROADMAP.md              # Development roadmap 
├── DEVELOPMENT_RULES.md    # Coding standards and rules
├── INTERFACES.md           # Component interfaces
├── STATUS.md               # Current status of development
├── DECISIONS.md            # Design decisions log
├── boot/                   # Bootloader configuration  
│   └── limine.cfg          # Limine bootloader config
├── kernel/                 # Kernel source code  
│   ├── src/
│   │   ├── lib.rs          # Main entry point and initialization 
│   │   ├── memory.rs       # Memory management
│   │   ├── paging.rs       # Paging implementation
│   │   ├── interrupt.rs    # Interrupt handling
│   │   └── diagnostics.rs  # Diagnostic/logging functions
│   └── Cargo.toml          # Kernel build configuration  
├── drivers/                # Hardware driver implementations
├── filesystem/
│   └── zenith-fs/          # Custom filesystem code
├── userland/               # User-space applications and tools
├── libraries/              # Shared libraries
├── gui/
│   └── zenith-ui/          # Graphical environment
├── tools/                  # Development tools
├── tests/                  # Test suite
└── docs/                   # Documentation  
```

## Kernel Components

### Aurora runtime accounting and native desktop

The cooperative scheduler attributes timestamp-counter deltas to the outgoing
task on the boot CPU; a snapshot includes the current unfinished slice. Zenith
Monitor compares consecutive snapshots at a configured interval. Interrupt
and switch overhead are attributed to the interrupted/current task. These
are residency estimates, not instruction counts or per-core hardware counters.
The globally locked free-list allocator reports occupied bytes, including
padding. Disk statistics are read from mounted metadata without a disk scan.

Settings and Monitor are kernel-hosted clients of the compositor. Shared
chart rendering is in `zenith_ui::chart`; see the
[Aurora plan](docs/aurora-development.md) for desktop ABI dependencies.

In opt-in `zenapp` mode, a static ELF starts at CPL3 before the compositor is
initialized, using the runner's dedicated page tables and kernel trap stack.
Syscalls copy bounded buffers via the runner's mapping checks. Notification
history is a fixed-size kernel service shared with the later Control Center;
it does not require a framebuffer during posting. See the
[application ABI](docs/native-app-abi.md) for the supported interface.

### Memory Management 
The kernel implements basic memory management including:
- Physical and virtual memory mapping
- Page table structures for x86_64 architecture  
- Heap allocation system

### Interrupt Handling
Supports:
- IDT (Interrupt Descriptor Table) setup
- Exception handling (0-31 interrupts)
- Basic interrupt controller support  

### Process Management  
Initial implementation includes:
- Simple process structure with states
- Basic scheduler framework 
- PID management

### Diagnostics System
Provides logging capabilities for kernel debugging and monitoring.

## Development Environment 

- **Target Architecture**: x86_64
- **Primary Language**: Rust
- **Bootloader**: Limine (UEFI compatible)  
- **Testing Platform**: QEMU virtual machine
- **Build System**: Cargo

This architecture provides a solid foundation for building the Zenith OS kernel while maintaining modularity and extensibility.
