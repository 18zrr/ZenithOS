/* Linker script for Zenith OS kernel (x86_64 physical load at 0x100000) */

ENTRY(_start)

PHDRS
{
    text PT_LOAD FLAGS(5);   /* read + execute */
    note PT_NOTE FLAGS(4);   /* read */
}

SECTIONS
{
    . = 0x100000;

    .multiboot2 : ALIGN(8) {
        *(.multiboot2)
    } :text

    .note.Xen : ALIGN(4) {
        *(.note.Xen)
    } :note

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
    }
}
