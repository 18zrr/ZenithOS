/* Linker script for the Limine native-protocol kernel. */

ENTRY(limine_start)

PHDRS
{
    text PT_LOAD FLAGS(7);   /* RWX: request responses and .data must be writable */
}

SECTIONS
{
    . = 0xFFFFFFFF80000000;

    .limine_requests_start : AT(0x100000) ALIGN(8) {
        *(.limine_requests_start)
    } :text

    .limine_requests : ALIGN(8) {
        *(.limine_requests)
    } :text

    .limine_requests_end : ALIGN(8) {
        *(.limine_requests_end)
    } :text

    .text : ALIGN(4096) {
        *(.text .text.*)
    } :text

    .rodata : ALIGN(4096) {
        *(.rodata .rodata.*)
    } :text

    .data : ALIGN(4096) {
        *(.data .data.*)
    } :text

    .bss : ALIGN(4096) {
        *(.bss .bss.*)
        *(COMMON)
    } :text

    /DISCARD/ : {
        *(.note.GNU-stack)
        *(.comment)
        *(.multiboot2)
        *(.note.Xen)
    }
}
