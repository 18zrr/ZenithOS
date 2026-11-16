# Zenith OS x86_64 boot entry.
#
# QEMU's direct `-kernel` path enters through the PVH ABI in 32-bit protected
# mode with paging enabled. This trampoline builds identity page tables,
# switches to long mode, then calls the Rust kernel. Limine loads the same ELF
# through its own protocol and ignores the Xen note.

# Multiboot2 header, used by Limine's `multiboot2` protocol. It must be within
# the first 32 KiB of the image and 8-byte aligned.
.section .multiboot2,"a",@progbits
.balign 8
mb2_start:
    .long 0xE85250D6                # magic
    .long 0                         # architecture: i386
    .long mb2_end - mb2_start       # header length
    .long -(0xE85250D6 + (mb2_end - mb2_start))  # checksum
    # Request a 1024x768x32 linear framebuffer (Multiboot2 tag type 8).
    .balign 8
    .long 8                         # framebuffer request tag
    .long 20                        # size
    .long 1024                      # width
    .long 768                       # height
    .long 32                        # bits per pixel
    .balign 8
    .short 0                        # end tag type
    .short 0                        # flags
    .long 8                         # size
mb2_end:

# PVH entry note: XEN_ELFNOTE_PHYS32_ENTRY (type 18), descriptor = entry addr.
.section .note.Xen,"",@note
.balign 4
.long 1f - 0f
.long 3f - 2f
.long 18
0: .asciz "Xen"
1: .balign 4
2: .quad _start
3: .balign 4

.code32
.section .text
.global _start
.extern kernel_main

_start:
    cld

    # Multiboot2 enters with EAX = magic and EBX = info structure address.
    # Save both so Rust can parse the framebuffer tag after the long-mode swap.
    mov %eax, mb2_magic
    mov %ebx, mb2_info_ptr
    cmp $0x36D76289, %eax   # Multiboot2 boot magic in EAX
    jne 1f
    movl $1, mb2_valid
1:
    lgdt gdtr

    # 32-bit code selector 0x08, data selector 0x10.
    ljmp $0x08, $1f
1:
    mov $0x10, %eax
    mov %eax, %ds
    mov %eax, %es
    mov %eax, %ss

    # Enable PAE and SSEFXSR (XMM instructions are emitted by Rust), then
    # long mode via EFER.LME.
    mov %cr4, %eax
    or $((1 << 5) | (1 << 9)), %eax
    mov %eax, %cr4

    mov $0xC0000080, %ecx
    rdmsr
    or $(1 << 8), %eax
    wrmsr

    # Identity-map 0-4 GiB with 2 MiB pages.
    mov $pml4, %eax
    mov %eax, %cr3

    # Paging was already enabled by the PVH loader; keep it enabled.
    mov %cr0, %eax
    or $(1 << 31), %eax
    mov %eax, %cr0

    # Far jump to 64-bit code selector 0x18.
    ljmp $0x18, $enter64

.code64
enter64:
    mov $0x10, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %ss

    # Stack at 0x400000 (above the 0x100000 image and the 0x200000-0x300000
    # kernel heap).
    mov $0x400000, %rsp
    and $-16, %rsp

    call kernel_main

2:
    cli
    hlt
    jmp 2b

.section .data
.balign 8
.global mb2_info_ptr
.global mb2_valid
.global mb2_magic
mb2_info_ptr: .quad 0
mb2_magic:    .quad 0
mb2_valid:    .long 0

# Limine request symbols are unused under PVH/Multiboot2 but referenced by
# shared Rust code; provide zeroed definitions.
.balign 8
.global limine_framebuffer_request
.global limine_rsdp_request
.global limine_memmap_request
.global limine_hhdm_request
limine_framebuffer_request: .quad 0,0,0,0,0,0
limine_rsdp_request:        .quad 0,0,0,0,0,0
limine_memmap_request:      .quad 0,0,0,0,0,0
limine_hhdm_request:        .quad 0,0,0,0,0,0
.balign 2
.global limine_cs
limine_cs: .word 0x18

.balign 16
gdt:
    .quad 0                         # null
    .quad 0x00CF9A000000FFFF        # 0x08: 32-bit code
    .quad 0x00CF92000000FFFF        # 0x10: data
    .quad 0x00AF9A000000FFFF        # 0x18: 64-bit code (L bit set)
    .quad 0                         # 0x20: reserved
    .quad 0x00CFF2000000FFFF        # 0x28: user data (DPL 3)
    .quad 0x00AFFA000000FFFF        # 0x30: user code (DPL 3, long mode)
gdt_end:

gdtr:
    .short gdt_end - gdt - 1
    .long gdt

.balign 4096
pml4:
    .quad pdpt + 7

.balign 4096
pdpt:
    .quad pd + 0x0000 + 7
    .quad pd + 0x1000 + 7
    .quad pd + 0x2000 + 7
    .quad pd + 0x3000 + 7

.balign 4096
pd:
    i = 0
    .rept 2048
    .quad 0x1e7 | (i << 21)
    i = i + 1
    .endr

.section .note.GNU-stack,"",@progbits
