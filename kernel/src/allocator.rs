//! Reclaiming allocator for the fixed kernel heap, including compositor surfaces.
use core::alloc::{GlobalAlloc, Layout};
use core::ptr::{self, NonNull};
use linked_list_allocator::Heap;

use crate::{interrupt::without_interrupts, sync::SpinLock};

pub const HEAP_SIZE: usize = 128 * 1024 * 1024;

#[repr(align(4096))]
struct Arena([u8; HEAP_SIZE]);

static mut ARENA: Arena = Arena([0; HEAP_SIZE]);
static HEAP: SpinLock<Heap> = SpinLock::new(Heap::empty());

struct KernelAllocator;

unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.size() > HEAP_SIZE || layout.align() > HEAP_SIZE {
            return ptr::null_mut();
        }
        without_interrupts(|| {
            let mut heap = HEAP.lock();
            if heap.size() == 0 {
                // One permanent, page-aligned backing region. Only this
                // lock's owner initializes or manipulates its free list.
                heap.init(ptr::addr_of_mut!(ARENA.0).cast(), HEAP_SIZE);
            }
            heap.allocate_first_fit(layout)
                .map_or(ptr::null_mut(), NonNull::as_ptr)
        })
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // GlobalAlloc callers supply the live pointer and its original layout.
        without_interrupts(|| {
            HEAP.lock()
                .deallocate(NonNull::new_unchecked(pointer), layout)
        });
    }
}

#[global_allocator]
static ALLOCATOR: KernelAllocator = KernelAllocator;

/// Bytes currently occupied, including the allocator's alignment overhead.
pub fn used() -> usize {
    without_interrupts(|| HEAP.lock().used())
}
