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
    __rodata_start = .;
    . = ALIGN(0x1000);
    /*
     * Dormant GPU experiments remain part of the build artifact even when no
     * production submission path references them. This makes "parked" mean
     * available for explicit experiments, not silently removed by --gc-sections.
     */
    KEEP(*(.gpgpu_artifacts))
    *(.rodata*)
    . = ALIGN(0x1000);
    __rodata_end = .;
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
    __data_start = .;
    . = ALIGN(0x1000);
    *(.data*)
    . = ALIGN(0x1000);
    __data_end = .;
  } :text

  .limine_requests : AT(ADDR(.limine_requests) - KERNEL_OFFSET) {
    __limine_requests_start = .;
    KEEP(*(.limine_requests))
    __limine_requests_end = .;
  } :text

  /*
   * Keep all file-backed GOT variants ahead of .bss. If one is left as an
   * orphan after .bss, even a zero-sized section can extend PT_LOAD's file
   * range across the entire otherwise-NOBITS .bss allocation.
   */
  .got : AT(ADDR(.got) - KERNEL_OFFSET) {
    *(.got)
    *(.igot)
  } :text

  .got.plt : AT(ADDR(.got.plt) - KERNEL_OFFSET) {
    *(.got.plt)
    *(.igot.plt)
  } :text

  /*
   * A candidate kernel publishes a tiny fixed ABI manifest. The old
   * generation validates this before it replaces the kernel PML4 slot.
   */
  .live_update_handoff ALIGN(64) : AT(ADDR(.live_update_handoff) - KERNEL_OFFSET) {
    __live_update_handoff_start = .;
    KEEP(*(.live_update_handoff))
    __live_update_handoff_end = .;
  } :text

  .live_update_slot ALIGN(8) : AT(ADDR(.live_update_slot) - KERNEL_OFFSET) {
    QUAD(0x545255454F534C55); /* TRUEOSLU */
    QUAD(0x4C49564555504454); /* LIVEUPDT */
    QUAD(2);                  /* ABI version */
    QUAD(trueos_live_update_ap_entry);
    QUAD(__live_update_handoff_start);
    QUAD(__live_update_handoff_end - __live_update_handoff_start);
  } :text

  /* Copied verbatim into a temporary PML4 slot during FULLFORGET. */
  .live_update_trampoline ALIGN(16) : AT(ADDR(.live_update_trampoline) - KERNEL_OFFSET) {
    __live_update_trampoline_start = .;
    KEEP(*(.live_update_trampoline))
    __live_update_trampoline_end = .;
  } :text

  .bss : AT(ADDR(.bss) - KERNEL_OFFSET) {
    __bss_start = .;
    . = ALIGN(0x1000);
    *(COMMON)
    *(.bss*)
    /*
     * Force this output section to be PROGBITS (file-backed) so it gets mapped
     * even if the loader ignores the PT_LOAD memsz>filesz tail.
     */
    BYTE(0);
    . = ALIGN(0x1000);
    __bss_end = .;
  } :text

  . = ALIGN(0x1000);
  kernel_end = .;
  kernel_end_phys = kernel_end - KERNEL_OFFSET;

  /DISCARD/ : {
    *(.eh_frame)
    *(.comment)
  }
}
