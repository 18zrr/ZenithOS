# Limine native boot protocol requests and 64-bit entry point.

.section .limine_requests_start,"a",@progbits
.balign 8
.quad 0xf6b8f4b39de7d1ae, 0xfab91a6940fcb9cf
.quad 0x785c6ed015d3e316, 0x181e920a7852b9d9

.section .limine_requests,"a",@progbits
.balign 8

.global limine_base_revision
limine_base_revision:
.quad 0xf9562b2d5c95a6c8, 0x6a7b384944536bdc, 0
.quad 0

.global limine_framebuffer_request
limine_framebuffer_request:
.quad 0xc7b1dd30df4c8b88, 0x0a82e883a194f07b
.quad 0x9d5827dcd881dd75, 0xa3148604f6fab11b
.quad 0
.quad 0

.global limine_rsdp_request
limine_rsdp_request:
.quad 0xc7b1dd30df4c8b88, 0x0a82e883a194f07b
.quad 0xc5e77b6b397e7b43, 0x27637845accdcf3c
.quad 0
.quad 0

.global limine_hhdm_request
limine_hhdm_request:
.quad 0xc7b1dd30df4c8b88, 0x0a82e883a194f07b
.quad 0x48dcf1cb8ad2b852, 0x63984e959a98244b
.quad 0
.quad 0

.global limine_memmap_request
limine_memmap_request:
.quad 0xc7b1dd30df4c8b88, 0x0a82e883a194f07b
.quad 0x67cf3d9d378a806f, 0xe304acdfc50c3c62
.quad 0
.quad 0

.global limine_kernel_address_request
limine_kernel_address_request:
.quad 0xc7b1dd30df4c8b88, 0x0a82e883a194f07b
.quad 0x71ba76863cc55f63, 0xb2644a48c516a487
.quad 0
.quad 0

.section .limine_requests_end,"a",@progbits
.balign 8
.quad 0xadc0e0531bb10d03, 0x9572709f31764c62

.section .text
.code64
.global limine_start
.extern kernel_main
.extern limine_cs
limine_start:
    cli
    # Rust emits SSE instructions; ensure FXSR is enabled.
    mov %cr4, %rax
    or $(1 << 9), %rax
    mov %rax, %cr4

    # Limine enters in long mode with a valid stack. Remember the code selector
    # so our IDT gates use the correct one.
    mov %cs, limine_cs(%rip)
    and $-16, %rsp
    call kernel_main
1:
    cli
    hlt
    jmp 1b

.section .data
.balign 2
.global limine_cs
limine_cs: .word 0x08

# Multiboot2 globals are unused under the native protocol, but the Rust code
# references them; provide zeroed definitions.
.balign 8
.global mb2_info_ptr
.global mb2_magic
.global mb2_valid
mb2_info_ptr: .quad 0
mb2_magic:    .quad 0
mb2_valid:    .long 0

.section .note.GNU-stack,"",@progbits
