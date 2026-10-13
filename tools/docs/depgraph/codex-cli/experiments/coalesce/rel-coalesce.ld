SECTIONS
{
  .text : { *(.text) *(.text.*) }
  .rodata : { *(.rodata) *(.rodata.*) }
  .data.rel.ro : { *(.data.rel.ro) *(.data.rel.ro.*) }
  .data : { *(.data) *(.data.*) }
  .bss : { *(.bss) *(.bss.*) *(COMMON) }
  .tdata : { *(.tdata) *(.tdata.*) }
  .tbss : { *(.tbss) *(.tbss.*) }
  .eh_frame : { *(.eh_frame) *(.eh_frame.*) }
  .gcc_except_table : { *(.gcc_except_table) *(.gcc_except_table.*) }
  .init_array : { *(.init_array) *(.init_array.*) }
  .fini_array : { *(.fini_array) *(.fini_array.*) }
}
