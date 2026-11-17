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

`kernel/src/allocator.rs` provides a global bump allocator over a static 2 MiB
region. It is sufficient for the kernel's `Vec`/`String` usage and does not
reclaim individual blocks.

## Current limits

- The page-fault recovery is special-cased to the `0x600000` test page.
- No general page allocator or arbitrary fault recovery exists.
- No kernel heap deallocation or compaction.
- No user/kernel address-space separation; userland is not running in the OS.

See [Kernel](Kernel.md) for the broader picture.
