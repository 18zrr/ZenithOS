//! Memory paging implementation for Zenith OS kernel

use crate::diagnostics::debug;

/// Page size (4KB as standard)
pub const PAGE_SIZE: usize = 4096;

/// Page table entry flags 
#[derive(Debug, Clone, Copy)]
pub struct PageTableEntry {
    pub present: bool,
    pub writable: bool,
    pub user_access: bool,
    pub accessed: bool,
    pub dirty: bool,
}

impl PageTableEntry {
    /// Create a new page table entry
    pub fn new() -> Self {
        PageTableEntry {
            present: false,
            writable: false,
            user_access: false,
            accessed: false,
            dirty: false,
        }
    }

    /// Convert to raw value for hardware use  
    pub fn as_u64(&self) -> u64 {
        let mut flags = 0u64;
        
        if self.present { flags |= 1; }
        if self.writable { flags |= 2; }
        if self.user_access { flags |= 4; } 
        if self.accessed { flags |= 32; }
        if self.dirty { flags |= 64; }
        
        flags
    }
}

/// Simple page allocator structure (for now)
pub struct PageAllocator {
    first_page: usize,
    last_page: usize,
    next_free_page: usize,
}

impl PageAllocator {
    /// Create new page allocator  
    pub const fn new(first_page: usize, last_page: usize) -> Self {
        PageAllocator {
            first_page,
            last_page,
            next_free_page: first_page,
        }
    }

    /// First page number managed by this allocator.
    pub const fn first_page(&self) -> usize {
        self.first_page
    }

    /// Allocate a page (simplified)
    pub fn allocate_page(&mut self) -> Option<usize> {
        if self.next_free_page < self.last_page {
            let addr = self.next_free_page;
            self.next_free_page += 1;
            Some(addr * PAGE_SIZE) // Convert page number to address
        } else {
            None // Out of pages
        }
    }

    /// Deallocate a page (placeholder)
    pub fn deallocate_page(&mut self, _addr: usize) {
        // Placeholder - in real implementation we'd track free pages
    }
}

/// Global page allocator instance 
static mut PAGE_ALLOCATOR: PageAllocator = PageAllocator::new(0, 0);

/// Initialize paging system  
pub fn init_paging() {
    unsafe {
        // Set up basic page allocation range (this would be determined by memory map)
        let first_page = 0x100;   // Start at page 256
        let last_page = 0x10000;  // Up to page 65536
        
        PAGE_ALLOCATOR = PageAllocator::new(first_page, last_page);
    }
    
    debug("Paging initialized");
}

/// Get a reference to the page allocator
#[allow(static_mut_refs)]
pub unsafe fn get_page_allocator() -> &'static mut PageAllocator {
    &mut PAGE_ALLOCATOR
}

/// Map a virtual address to physical address (placeholder).
///
/// Page-table manipulation is not implemented yet; the addresses are accepted
/// so callers can be written against the final interface.
pub fn map_virtual_to_physical(_virtual_addr: usize, _physical_addr: usize) {
    debug("map_virtual_to_physical: page tables not implemented yet");
}