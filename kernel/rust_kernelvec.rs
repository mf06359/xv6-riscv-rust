//! Supervisor-mode interrupt / exception entry vector.
//!
//! Replaces `kernelvec.S`. `stvec` is set to `kernelvec` while running in
//! the kernel; on a trap the CPU jumps here in supervisor mode. We push
//! the caller-saved registers onto the kernel stack, call `kerneltrap()`
//! (Rust), restore the registers, and return via `sret`.
//!
//! - The function must be 4-byte aligned: rustc places naked functions in
//!   their own `.text.<name>` section; the linker honors `.align 4` from
//!   the `naked_asm!`. We also add `.balign 4` explicitly to be safe.
//! - `tp` is intentionally NOT restored: it holds the hartid and may be
//!   stale if we got rescheduled across CPUs.
//! - `sp` is not stored/restored either — its stash slot at offset 8 is
//!   left as scratch because the kernel stack is implicit.

use core::arch::naked_asm;

#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kernelvec() {
    naked_asm!(
        ".balign 4",
        // make room to save registers.
        "addi sp, sp, -256",

        // save caller-saved registers.
        "sd ra,    0(sp)",
        // sp at offset 8 is scratch.
        "sd gp,   16(sp)",
        "sd tp,   24(sp)",
        "sd t0,   32(sp)",
        "sd t1,   40(sp)",
        "sd t2,   48(sp)",
        "sd a0,   72(sp)",
        "sd a1,   80(sp)",
        "sd a2,   88(sp)",
        "sd a3,   96(sp)",
        "sd a4,  104(sp)",
        "sd a5,  112(sp)",
        "sd a6,  120(sp)",
        "sd a7,  128(sp)",
        "sd t3,  216(sp)",
        "sd t4,  224(sp)",
        "sd t5,  232(sp)",
        "sd t6,  240(sp)",

        // call the kernel trap handler (implemented in rust_trap.rs).
        "call kerneltrap",

        // restore registers.
        "ld ra,    0(sp)",
        "ld gp,   16(sp)",
        // tp: not restored — may be stale if we moved CPUs.
        "ld t0,   32(sp)",
        "ld t1,   40(sp)",
        "ld t2,   48(sp)",
        "ld a0,   72(sp)",
        "ld a1,   80(sp)",
        "ld a2,   88(sp)",
        "ld a3,   96(sp)",
        "ld a4,  104(sp)",
        "ld a5,  112(sp)",
        "ld a6,  120(sp)",
        "ld a7,  128(sp)",
        "ld t3,  216(sp)",
        "ld t4,  224(sp)",
        "ld t5,  232(sp)",
        "ld t6,  240(sp)",

        "addi sp, sp, 256",

        // return to whatever we were doing in the kernel.
        "sret",
    );
}
