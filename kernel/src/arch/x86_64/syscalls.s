# System-call entry point for the `syscall`/`sysretq` instruction pair.

.section .text
.code64

.extern rust_syscall_handler

.global syscall_entry
.type syscall_entry, @function
syscall_entry:
    cli
    pushq %r15
    pushq %r14
    pushq %r13
    pushq %r12
    pushq %r11
    pushq %r10
    pushq %r9
    pushq %r8
    pushq %rbp
    pushq %rdi
    pushq %rsi
    pushq %rdx
    pushq %rcx
    pushq %rbx
    pushq %rax

    movq %rsp, %rdi
    call rust_syscall_handler

    popq %rax
    popq %rbx
    popq %rcx
    popq %rdx
    popq %rsi
    popq %rdi
    popq %rbp
    popq %r8
    popq %r9
    popq %r10
    popq %r11
    popq %r12
    popq %r13
    popq %r14
    popq %r15

    sti
    sysretq

.section .note.GNU-stack,"",@progbits
