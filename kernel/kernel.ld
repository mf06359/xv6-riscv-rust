OUTPUT_ARCH( "riscv" )
ENTRY( _entry )

SECTIONS
{
  /*
   * ensure that entry.S / _entry is at 0x80000000,
   * where qemu's -kernel jumps.
   */
  . = 0x80000000;

  .text : {
    /* `_entry` (rust_entry.rs) is placed first via its own
       .text._entry section so it ends up at 0x80000000. */
    *(.text._entry)
    *(.text .text.*)
    . = ALIGN(0x1000);
    _trampoline = .;
    /* `trampoline` is the symbol the Rust kernel computes offsets from.
       `uservec` is the first naked fn in trampsec (rust_trampoline.rs)
       so its address coincides with `trampoline` / `_trampoline`. */
    PROVIDE(trampoline = .);
    *(trampsec)
    . = ALIGN(0x1000);
    ASSERT(. - _trampoline == 0x1000, "error: trampoline larger than one page");
    PROVIDE(etext = .);
  }

  .rodata : {
    . = ALIGN(16);
    *(.srodata .srodata.*) /* do not need to distinguish this from .rodata */
    . = ALIGN(16);
    *(.rodata .rodata.*)
  }

  .data : {
    . = ALIGN(16);
    *(.sdata .sdata.*) /* do not need to distinguish this from .data */
    . = ALIGN(16);
    *(.data .data.*)
  }

  .bss : {
    . = ALIGN(16);
    *(.sbss .sbss.*) /* do not need to distinguish this from .bss */
    . = ALIGN(16);
    *(.bss .bss.*)
  }

  PROVIDE(end = .);
}
