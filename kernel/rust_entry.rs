//! Boot entry. Replaces `entry.S`.
//!
//! `_entry` is placed at physical address 0x80000000 by `kernel.ld` —
//! that's where qemu's `-kernel` flag jumps after loading the ELF. We are
//! still in machine mode here. The job is just:
//!
//! 1. Set the stack pointer to a per-hart slot inside `stack0`.
//! 2. Call into Rust (`start`) which finishes M-mode setup and switches
//!    the hart into S-mode.
//! 3. Spin if `start` ever returns (it shouldn't).
//!
//! `stack0` is declared in `rust_start.rs` as a page-aligned `[u8; NCPU*4096]`
//! array; each hart reserves the slice at `stack0 + (hartid+1)*4096`.

use core::arch::naked_asm;

#[unsafe(naked)]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text._entry")]
pub unsafe extern "C" fn _entry() {
    naked_asm!(
        "la sp, stack0",
        "li a0, 1024*4",
        "csrr a1, mhartid",
        "addi a1, a1, 1",
        "mul a0, a0, a1",
        "add sp, sp, a0",
        "call start",
        // Should be unreachable; spin if start returns somehow.
        "1: j 1b",
    );
}
