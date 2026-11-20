# Single-CPU CPL3 bring-up path, only linked with the native-run feature.
.section .data
.balign 16
.Lgdt:
    .quad 0, 0x00af9a000000ffff, 0x00cf92000000ffff
    .quad 0x00cff2000000ffff, 0x00affa000000ffff
.Ltss_desc:
    .quad 0,0
.Lgdtr:
    .word 55
    .quad .Lgdt
.balign 16
.Ltss:
    .long 0
    .quad .Lkstack_top              # RSP0
    .zero 90
    .word 104                       # no I/O permission bitmap
.section .bss
.balign 16
    .skip 65536
.Lkstack_top:
.Lsaved_rsp: .quad 0
.Lsaved_cr3: .quad 0
.Luser_rsp: .quad 0
.balign 16
.Lkernel_fx: .skip 512
.section .data
.balign 16
.Lclean_fx:
    .word 0x037f
    .zero 22
    .long 0x1f80
    .zero 484

.section .text
.global native_gdt
native_gdt:
    lea .Ltss(%rip), %rax
    lea .Ltss_desc(%rip), %rdx
    movw $103, (%rdx)
    movw %ax, 2(%rdx)
    shr $16, %rax
    movb %al, 4(%rdx)
    movb $0x89, 5(%rdx)
    movb %ah, 7(%rdx)
    shr $16, %rax
    movl %eax, 8(%rdx)
    lgdt .Lgdtr(%rip)
    pushq $8
    lea .Lnew_cs(%rip), %rax
    push %rax
    lretq
.Lnew_cs:
    mov $16, %eax
    mov %eax, %ds
    mov %eax, %es
    mov %eax, %ss
    mov $0x28, %eax
    ltr %ax
    movw $8, limine_cs(%rip)
    ret

.global native_enter
native_enter:                       # (rip, rsp, cr3); kernel has IF masked
    push %rbp
    push %rbx
    push %r12
    push %r13
    push %r14
    push %r15
    mov %rsp, .Lsaved_rsp(%rip)
    mov %cr3, %rax
    mov %rax, .Lsaved_cr3(%rip)
    fxsave64 .Lkernel_fx(%rip)
    fxrstor64 .Lclean_fx(%rip)
    pushq $0x1b
    push %rsi
    pushq $0x202
    pushq $0x23
    push %rdi
    mov %rdx, %cr3
    mov $0x1b, %eax
    mov %eax, %ds
    mov %eax, %es
    xor %eax, %eax
    xor %ebx, %ebx
    xor %ecx, %ecx
    xor %edx, %edx
    xor %esi, %esi
    xor %edi, %edi
    xor %ebp, %ebp
    xor %r8d, %r8d
    xor %r9d, %r9d
    xor %r10d, %r10d
    xor %r11d, %r11d
    xor %r12d, %r12d
    xor %r13d, %r13d
    xor %r14d, %r14d
    xor %r15d, %r15d
    iretq

.global native_syscall
native_syscall:
    # SYSCALL does not switch stacks. FMASK has already cleared IF and DF.
    mov %rsp, .Luser_rsp(%rip)
    lea .Lkstack_top(%rip), %rsp
    pushq $0x1b
    pushq .Luser_rsp(%rip)
    push %r11
    pushq $0x23
    push %rcx
    pushq $0
    pushq $256
    jmp isr_common

.global native_finish
native_finish:                      # (status); no live kernel guards
    cli
    cld
    mov .Lsaved_cr3(%rip), %rax
    mov %rax, %cr3
    mov .Lsaved_rsp(%rip), %rsp
    fxrstor64 .Lkernel_fx(%rip)
    mov $16, %eax
    mov %eax, %ds
    mov %eax, %es
    mov %rdi, %rax
    pop %r15
    pop %r14
    pop %r13
    pop %r12
    pop %rbx
    pop %rbp
    ret
.section .note.GNU-stack,"",@progbits
