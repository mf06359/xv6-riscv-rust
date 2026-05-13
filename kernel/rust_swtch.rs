//! Context switch primitive.
//!
//! Replaces the original `swtch.S`. The function saves the callee-saved
//! registers (ra, sp, s0..s11) of the outgoing context into `*old`, loads
//! them from `*new`, and returns. After the load, the program counter
//! resumes at the `ra` of the new context, so control transfers to the
//! procedure that previously called `swtch` from that context.
//!
//!     fn swtch(old: *mut Context, new: *mut Context);
//!
//! `Context` (defined in `rust_spinlock.rs`) is `#[repr(C)]` with the
//! field order `ra, sp, s0..s11`, so the byte offsets used below
//! (0, 8, 16, 24, ...) match the field layout exactly.

use core::arch::naked_asm;

#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn swtch() {
    naked_asm!(
        // Save outgoing context (a0 = old).
        "sd ra,    0(a0)",
        "sd sp,    8(a0)",
        "sd s0,   16(a0)",
        "sd s1,   24(a0)",
        "sd s2,   32(a0)",
        "sd s3,   40(a0)",
        "sd s4,   48(a0)",
        "sd s5,   56(a0)",
        "sd s6,   64(a0)",
        "sd s7,   72(a0)",
        "sd s8,   80(a0)",
        "sd s9,   88(a0)",
        "sd s10,  96(a0)",
        "sd s11, 104(a0)",
        // Load incoming context (a1 = new).
        "ld ra,    0(a1)",
        "ld sp,    8(a1)",
        "ld s0,   16(a1)",
        "ld s1,   24(a1)",
        "ld s2,   32(a1)",
        "ld s3,   40(a1)",
        "ld s4,   48(a1)",
        "ld s5,   56(a1)",
        "ld s6,   64(a1)",
        "ld s7,   72(a1)",
        "ld s8,   80(a1)",
        "ld s9,   88(a1)",
        "ld s10,  96(a1)",
        "ld s11, 104(a1)",
        "ret",
    );
}
