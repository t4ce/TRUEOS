/* Relocatable INIT/SIPI entry. This whole section is copied below 1 MiB.
 * The only absolute addresses are populated by smp::prepare(). There are
 * no assembler/linker relocations inside the copied range. */
.section .text$ap,"xr"
.balign 16
.global trueos_ap_trampoline_start
.global trueos_ap_trampoline_end
.global trueos_ap_far32
.global trueos_ap_gdtr32
.global trueos_ap_gdtr64
.global trueos_ap_parameters
.global trueos_ap_mode32
.global trueos_ap_gdt
trueos_ap_trampoline_start:
.code16
    cli
    cld
    mov ax, cs
    mov ds, ax
    mov es, ax
    xor ebx, ebx
    mov bx, ax
    shl ebx, 4

    /* lgdt with a 32-bit base and a CS-relative 16-bit displacement. */
    .byte 0x66, 0x0f, 0x01, 0x16
    .word trueos_ap_gdtr32 - trueos_ap_trampoline_start
    mov eax, 0x40010033
    mov cr0, eax
    /* jmp m16:32; DS still has the real-mode CS segment base. */
    .byte 0x66, 0xff, 0x2e
    .word trueos_ap_far32 - trueos_ap_trampoline_start

.code32
trueos_ap_mode32:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax
    xor eax, eax
    lldt ax

    /* Save our physical trampoline base across CPUID. */
    mov esi, ebx
    mov eax, 0
    cpuid
    cmp eax, 0x0b
    jb 1f
    mov eax, 0x0b
    xor ecx, ecx
    cpuid
    test ebx, ebx
    jz 1f
    mov edi, edx
    jmp 2f
1:
    mov eax, 1
    cpuid
    mov edi, ebx
    shr edi, 24
2:
    mov ebx, esi
    mov ebp, dword ptr [ebx + AP_PARAMETERS + 32]
    mov ecx, dword ptr [ebx + AP_PARAMETERS + 36]
3:
    test ecx, ecx
    jz 99f
    cmp dword ptr [ebp], edi
    je 4f
    add ebp, 32
    dec ecx
    jmp 3b
4:
    /* Match the BSP's APIC mode without changing its configured base. */
    mov ecx, 0x1b
    rdmsr
    test eax, 0x400
    jz 5f
    test dword ptr [ebx + AP_PARAMETERS + 8], 0x400
    jnz 5f
    and eax, 0xfffff3ff
    wrmsr
5:
    mov eax, dword ptr [ebx + AP_PARAMETERS + 8]
    mov edx, dword ptr [ebx + AP_PARAMETERS + 12]
    and eax, 0xfffffeff
    or eax, 0x800
    wrmsr

    mov ecx, 0x277
    mov eax, dword ptr [ebx + AP_PARAMETERS + 24]
    mov edx, dword ptr [ebx + AP_PARAMETERS + 28]
    wrmsr
    mov eax, 0x620
    mov cr4, eax
    mov ecx, 0xc0000080
    mov eax, dword ptr [ebx + AP_PARAMETERS + 16]
    xor edx, edx
    wrmsr
    mov eax, dword ptr [ebx + AP_PARAMETERS]
    mov cr3, eax
    mov eax, cr0
    or eax, 0x80000000
    mov cr0, eax
    /* Far jump to a 64-bit segment without requiring an absolute relocation. */
    lea eax, [ebx + AP_MODE64]
    mov edx, 0x08
    /* A real stack is not needed until now; its physical low half is valid. */
    mov esp, dword ptr [ebp + 4]
    push edx
    push eax
    retf
99:
    cli
    hlt
    jmp 99b

.code64
.Lmode64:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax
    mov ebx, ebx
    mov ebp, ebp
    lgdt [rbx + AP_GDTR64]
    mov rax, qword ptr [rbx + AP_PARAMETERS + 40]
    lea rdx, [rbx + AP_HIGH]
    add rdx, rax
    add rbp, rax
    add rbx, rax
    mov rsp, qword ptr [rbp + 16]
    jmp rdx
.Lhigh:
    /* INIT preserves MTRRs on many CPUs, but copying the BSP snapshot makes
     * the cache policy explicit and identical on every P/E core and thread.
     * Caches are disabled, interrupts are off, and no AP accesses shared data
     * with caches enabled until this sequence is complete. */
    wbinvd
    mov esi, dword ptr [rbx + AP_PARAMETERS + 48]
    add rsi, qword ptr [rbx + AP_PARAMETERS + 40]
    cmp dword ptr [rsi + 4], 0
    je 8f
    mov ecx, 0x2ff
    rdmsr
    and eax, 0xfffff7ff
    wrmsr
    mov edi, dword ptr [rsi]
    add rsi, 8
6:
    test edi, edi
    jz 8f
    mov ecx, dword ptr [rsi]
    mov eax, dword ptr [rsi + 8]
    mov edx, dword ptr [rsi + 12]
    wrmsr
    add rsi, 16
    dec edi
    jmp 6b
8:
    wbinvd
    mov rax, cr3
    mov cr3, rax
    mov rax, cr0
    btr rax, 30
    btr rax, 29
    mov cr0, rax
    xor eax, eax
    mov ecx, 0xc0000100
    xor edx, edx
    wrmsr
    inc ecx
    wrmsr
    inc ecx
    wrmsr
    mov rdi, qword ptr [rbp + 8]
    lock or dword ptr [rbp + 24], 1
9:
    mov rax, qword ptr [rdi + 16]
    test rax, rax
    jnz 10f
    pause
    jmp 9b
10:
    mov rdx, cr3
    mov cr3, rdx
    mov rsp, qword ptr [rbp + 16]
    and rsp, -16
    sub rsp, 8
    mov qword ptr [rsp], 0
    xor ebp, ebp
    xor ebx, ebx
    xor ecx, ecx
    xor edx, edx
    xor esi, esi
    xor r8d, r8d
    xor r9d, r9d
    xor r10d, r10d
    xor r11d, r11d
    xor r12d, r12d
    xor r13d, r13d
    xor r14d, r14d
    xor r15d, r15d
    jmp rax

.balign 8
trueos_ap_far32:
    .long 0
    .word 0x18
trueos_ap_gdtr32:
    .word 31
    .long 0
trueos_ap_gdtr64:
    .word 31
    .quad 0
.balign 8
trueos_ap_gdt:
    .quad 0
    .quad 0x00af9a000000ffff
    .quad 0x00cf92000000ffff
    .quad 0x00cf9a000000ffff
.balign 8
trueos_ap_parameters:
    .zero 56
trueos_ap_trampoline_end:
.code64

.set AP_PARAMETERS, trueos_ap_parameters - trueos_ap_trampoline_start
.set AP_GDTR64, trueos_ap_gdtr64 - trueos_ap_trampoline_start
.set AP_MODE64, .Lmode64 - trueos_ap_trampoline_start
.set AP_HIGH, .Lhigh - trueos_ap_trampoline_start
