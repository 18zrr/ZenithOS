//! Memory management for Zenith OS kernel

use crate::diagnostics::debug;

/// Physical memory region structure
#[derive(Debug, Clone, Copy)]
pub struct MemoryRegion {
    pub start: usize,
    pub end: usize,
    pub size: usize,
}

impl MemoryRegion {
    /// Create a new memory region
    pub fn new(start: usize, end: usize) -> Self {
        MemoryRegion {
            start,
            end,
            size: end - start,
        }
    }
}

/// Simple memory allocator structure 
pub struct MemoryAllocator {
    heap_start: usize,
    heap_end: usize,
    next_free_addr: usize,
}

impl MemoryAllocator {
    /// Create new memory allocator
    pub const fn new(heap_start: usize, heap_end: usize) -> Self {
        MemoryAllocator {
            heap_start,
            heap_end,
            next_free_addr: heap_start,
        }
    }

    /// Allocate memory (simplified)
    pub fn allocate(&mut self, size: usize) -> Option<usize> {
        // Align to an 8-byte boundary without overflowing on huge requests.
        let aligned_size = match size.checked_add(7) {
            Some(value) => value & !7,
            None => return None,
        };

        match self.next_free_addr.checked_add(aligned_size) {
            Some(next) if next <= self.heap_end => {
                let addr = self.next_free_addr;
                self.next_free_addr = next;
                Some(addr)
            }
            _ => None, // Out of memory
        }
    }

    /// Deallocate memory (placeholder for now - just moves the pointer back)
    pub fn deallocate(&mut self, _addr: usize, _size: usize) {
        // For simplicity in this basic implementation, we don't track actual free blocks
        // In a real system, this would mark blocks as available
    }

    /// Get current heap usage statistics  
    pub fn get_stats(&self) -> (usize, usize, usize) {
        let used = self.next_free_addr - self.heap_start;
        let remaining = self.heap_end - self.next_free_addr;
        (used, remaining, self.heap_end - self.heap_start)
    }
}

/// Global memory allocator instance  
static mut MEMORY_ALLOCATOR: MemoryAllocator = MemoryAllocator::new(0, 0);

/// Initialize memory management system
pub fn init_memory_management() {
    unsafe {
        // In a real implementation this would parse the memory map from ACPI.
        // For now use a fixed heap that does not overlap the kernel image at
        // 0x100000 or the stack at 0x400000.
        let heap_start = 0x200000;
        let heap_end = 0x300000;
        
        MEMORY_ALLOCATOR = MemoryAllocator::new(heap_start, heap_end);
    }
    
    debug("Memory management initialized");
}

/// Get a reference to the global memory allocator
#[allow(static_mut_refs)]
pub unsafe fn get_memory_allocator() -> &'static mut MemoryAllocator {
    &mut MEMORY_ALLOCATOR
}