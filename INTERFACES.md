# Zenith OS Interfaces

## Overview
This document specifies the interfaces between various components of Zenith OS. These interfaces define contracts, data structures, and protocols that ensure modularity and maintainability.

## Kernel Interface Contracts

For the implemented native app entry point, use the
[Zenith x86_64 ABI v1](docs/native-app-abi.md). The conceptual interfaces
below are not the syscall numbers of the `zenapp` dispatcher.

### System Call Interface
```
fn syscall_handler(id: u64, args: [u64; 6]) -> u64 {
    // Handle system calls from userland
    match id {
        0 => sys_read(args[0], args[1], args[2]),
        1 => sys_write(args[0], args[1], args[2]),
        2 => sys_open(args[0], args[1]),
        3 => sys_close(args[0]),
        _ => 0xFFFFFFFFFFFFFFFF // Invalid syscall
    }
}
```

### Memory Management Interface
```rust
pub struct MemoryRegion {
    pub start: usize,
    pub end: usize,
    pub flags: MemoryFlags,
}

pub enum MemoryFlags {
    Readable,
    Writable, 
    Executable,
    Present,
}
```

### Interrupt Handler Interface
```rust
pub trait InterruptHandler {
    fn handle(&mut self, interrupt_number: u8);
}
```

## Filesystem Interface

### Zenith FS Core Interface
```rust
pub struct FileSystem {
    // File system specific data
}

impl FileSystem {
    pub fn create_file(&mut self, path: &str) -> Result<FileHandle, FileSystemError>;
    pub fn read_file(&self, handle: FileHandle) -> Result<Vec<u8>, FileSystemError>;
    pub fn write_file(&mut self, handle: FileHandle, data: &[u8]) -> Result<(), FileSystemError>;
    pub fn delete_file(&mut self, path: &str) -> Result<(), FileSystemError>;
}
```

### File Handle Interface
```rust
pub struct FileHandle {
    pub inode: u64,
    pub position: usize,
    pub flags: OpenFlags,
}

#[derive(Clone)]
pub enum OpenFlags {
    Read,
    Write,
    ReadWrite,
}
```

## Driver Interface

### Device Driver Contract  
```rust
pub trait Driver {
    fn initialize(&mut self) -> Result<(), DriverError>;
    fn handle_interrupt(&mut self, irq: u8);
    fn get_device_info(&self) -> DeviceInfo;
}

pub struct DeviceInfo {
    pub device_type: DeviceType,
    pub vendor_id: u16,
    pub product_id: u16,
}
```

## Graphics Interface

### Framebuffer Interface
```rust
pub struct Framebuffer {
    pub width: usize,
    pub height: usize,
    pub pitch: usize,
    pub buffer: *mut u8,
}

impl Framebuffer {
    pub fn clear(&mut self, color: Color);
    pub fn draw_pixel(&mut self, x: usize, y: usize, color: Color);
    pub fn get_pixel(&self, x: usize, y: usize) -> Color;
}
```

## User Interface Contract

### Window Manager Interface
```rust
pub struct WindowManager {
    pub windows: Vec<Window>,
}

impl WindowManager {
    pub fn create_window(&mut self, title: &str, width: usize, height: usize) -> WindowHandle;
    pub fn render(&self);
}
```

## Communication Protocols

### Kernel-Userland Protocol
All communication between kernel and user applications occurs through:
1. System calls (via syscall interface)
2. Shared memory regions for large data transfers  
3. Message queues for asynchronous operations

### Driver Communication
Drivers communicate with the kernel via:
1. Device-specific interfaces 
2. Interrupt mechanisms for real-time events
3. Memory mapping for direct hardware access

## Error Handling Interface

All components use a standardized error type system:
```rust
pub enum ZenithError {
    IoError(IoError),
    FileSystemError(FileSystemError),
    MemoryError(MemoryError),
    DriverError(DriverError),
    NotImplemented,
}
```

## Version Compatibility
- Interfaces are versioned for backward compatibility
- Breaking changes require explicit major version increments  
- All interfaces must be documented and tested

This interface specification ensures that components of Zenith OS can be developed, tested, and maintained independently while maintaining system coherence.
