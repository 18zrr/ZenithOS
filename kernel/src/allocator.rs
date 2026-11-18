//! Simple bump allocator providing `alloc` support for kernel components.

use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicUsize, Ordering};

// Wallpaper, composition and previous-frame buffers need ~95 MiB at 4K.
// Include headroom for filesystem and other kernel allocations.
pub const HEAP_SIZE: usize = 128 * 1024 * 1024;

static mut HEAP: [u8; HEAP_SIZE] = [0; HEAP_SIZE];
static OFFSET: AtomicUsize = AtomicUsize::new(0);

struct BumpAllocator;

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut current = OFFSET.load(Ordering::Relaxed);
        loop {
            let aligned = (current + layout.align() - 1) & !(layout.align() - 1);
            let end = match aligned.checked_add(layout.size()) {
                Some(end) => end,
                None => return core::ptr::null_mut(),
            };
            if end > HEAP_SIZE {
                return core::ptr::null_mut();
            }
            match OFFSET.compare_exchange_weak(
                current,
                end,
                Ordering::SeqCst,
                Ordering::Relaxed,
            ) {
                Ok(_) => return (core::ptr::addr_of_mut!(HEAP) as *mut u8).add(aligned),
                Err(value) => current = value,
            }
        }
    }

    unsafe fn dealloc(&self, _pointer: *mut u8, _layout: Layout) {
        // Bump allocation does not reclaim individual blocks.
    }
}

#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator;

/// Number of bytes handed out so far.
pub fn used() -> usize {
    OFFSET.load(Ordering::Relaxed)
}
