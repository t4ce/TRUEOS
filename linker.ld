ENTRY(_start)

KERNEL_OFFSET = 0xffffffff80000000;
KERNEL_PHYS_BASE = 0x00100000;

PHDRS {
  headers PT_PHDR PHDRS FLAGS(0x4); /* Map the program header table itself. */
  text    PT_LOAD FILEHDR PHDRS FLAGS(0x7); /* Map ELF header + PHDRs in higher half. */
}

SECTIONS
{
  /* Higher-half kernel mapped at KERNEL_OFFSET, loaded at KERNEL_PHYS_BASE. */
  . = KERNEL_OFFSET + KERNEL_PHYS_BASE;

  . = ALIGN(0x1000);
  .text : AT(ADDR(.text) - KERNEL_OFFSET) {
    __text_start = .;
    . = ALIGN(0x1000);
    *(.text*)
    . = ALIGN(0x1000);
    __text_end = .;
  } :text

  .rodata : AT(ADDR(.rodata) - KERNEL_OFFSET) {
    . = ALIGN(0x1000);
    *(.rodata*)
    . = ALIGN(0x1000);
  } :text

  .debug_gdb_scripts : AT(ADDR(.debug_gdb_scripts) - KERNEL_OFFSET) {
    KEEP(*(.debug_gdb_scripts))
  } :text

  .eh_frame_hdr : AT(ADDR(.eh_frame_hdr) - KERNEL_OFFSET) {
    KEEP(*(.eh_frame_hdr))
  } :text

  .text_padding : AT(ADDR(.text_padding) - KERNEL_OFFSET) {
    . = ALIGN(0x1000);
    LONG(0);
  } :text

  . = ALIGN(0x1000);

  .data : AT(ADDR(.data) - KERNEL_OFFSET) {
    . = ALIGN(0x1000);
    *(.data*)
    . = ALIGN(0x1000);
  } :text

  .limine_requests : AT(ADDR(.limine_requests) - KERNEL_OFFSET) {
    KEEP(*(.limine_requests))
  } :text

  .bss : AT(ADDR(.bss) - KERNEL_OFFSET) {
    . = ALIGN(0x1000);
    *(COMMON)
    *(.bss*)
    /*
     * Force this output section to be PROGBITS (file-backed) so it gets mapped
     * even if the loader ignores the PT_LOAD memsz>filesz tail.
     */
    BYTE(0);
    . = ALIGN(0x1000);
  } :text

  . = ALIGN(0x1000);
  kernel_end = .;
  kernel_end_phys = kernel_end - KERNEL_OFFSET;

  /DISCARD/ : {
    *(.eh_frame)
    *(.comment)
  }
}
