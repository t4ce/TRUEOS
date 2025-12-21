ENTRY(_start)

KERNEL_OFFSET = 0xffffffff80000000;

SECTIONS
{
  . = KERNEL_OFFSET; 

  . = ALIGN(0x1000);
  .text : {
    __text_start = .;
    . = ALIGN(0x1000);
    *(.text*)
    . = ALIGN(0x1000);
    __text_end = .;
  } :text

  .rodata : {
    . = ALIGN(0x1000);
    *(.rodata*)
    . = ALIGN(0x1000);
  } :text

  .debug_gdb_scripts : {
    KEEP(*(.debug_gdb_scripts))
  } :text

  .eh_frame_hdr : {
    KEEP(*(.eh_frame_hdr))
  } :text

  .text_padding : {
    . = ALIGN(0x1000);
    LONG(0);
  } :text

  . = ALIGN(0x1000);

  .data : {
    . = ALIGN(0x1000);
    *(.data*)
    . = ALIGN(0x1000);
  } :text

  .limine_reqs : {
    KEEP(*(.limine_reqs.start))
    KEEP(*(.limine_reqs.entries))
    KEEP(*(.limine_reqs.end))
  } :text

  .bss : {
    . = ALIGN(0x1000);
    *(COMMON)
    *(.bss*)
    . = ALIGN(0x1000);
  } :text

  . = ALIGN(0x1000);
  kernel_end = .;

  /DISCARD/ : {
    *(.eh_frame)
    *(.comment)
  }
}

PHDRS
{
  text PT_LOAD FLAGS(0x7);   /* RWX */
}
