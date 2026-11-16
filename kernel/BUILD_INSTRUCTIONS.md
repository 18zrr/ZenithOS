# Building Zenith OS Kernel

## Prerequisites

To build the Zenith OS kernel, you need:

1. Rust toolchain (stable version)
2. Cross-compilation target: `x86_64-unknown-none`

## Setup Instructions

### Install Required Toolchain
```bash
# Install rustup if not already installed
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Add the required target
rustup target add x86_64-unknown-none
```

### Build Process
```bash
cd kernel/
cargo build --target x86_64-unknown-none --release
```

## Current Implementation Status

The current implementation in `src/lib.rs` includes:
- Proper `_start` entry point for UEFI boot
- Standard no_std configuration 
- Panic handler that loops indefinitely
- Basic kernel structure ready for further development

## File Structure
```
kernel/
├── src/
│   ├── lib.rs          # Main kernel implementation
│   ├── memory.rs       # Memory management  
│   ├── paging.rs       # Paging implementation
│   └── diagnostics.rs  # Debug output functions
├── Cargo.toml          # Rust package manifest
└── build_kernel.sh     # Build script
```

## Testing

Once properly built, the kernel can be tested in QEMU:
```bash
qemu-system-x86_64 -kernel kernel.bin -serial stdio
```

Note: This requires additional setup of a bootloader and proper UEFI environment.