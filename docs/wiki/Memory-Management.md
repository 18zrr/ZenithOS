# Memory Management

## Page tables

`kernel/src/page_table.rs` builds four-level x86_64 page tables with identity
mappings and a higher-half alias. The kernel switches CR3 to these tables on
the legacy/PVH path. On the native Limine path it keeps Limine's page tables
and uses the reported HHDM.

## Demand paging test

A 4 KiB page table is prepared with an unmapped first entry. The kernel writes
`0xDEADBEEF` to `0x600000`, which faults; the page-fault handler maps the page
and returns, and the instruction re-executes successfully. A read then returns
the written value. This is a verified test in QEMU.

## Heap allocator

`kernel/src/allocator.rs` manages a fixed 128 MiB region with an aligned,
coalescing free list. Allocation and deallocation mask interrupts while the
heap lock is held. `tasks` reports occupied heap bytes, including alignment.

## Current limits

- The page-fault recovery is special-cased to the `0x600000` test page.
- No general page allocator or arbitrary fault recovery exists.
- No compaction or dynamic expansion of the kernel heap.
- No user/kernel address-space separation; userland is not running in the OS.

See [Kernel](Kernel.md) for the broader picture.
