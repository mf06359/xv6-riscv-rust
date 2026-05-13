//! User <-> kernel transition trampoline. Replaces `trampoline.S`.
//!
//! This page lives at virtual address `TRAMPOLINE` in **both** kernel and
//! user page tables (mapped via `kernel.ld`'s `trampsec` section, aligned
//! to one page, asserted to be exactly 4096 bytes). Because the page is
//! mapped identically in both address spaces, instructions can keep
//! executing across a `csrw satp` page-table swap without faulting.
//!
//! The page exposes three global symbols, all in the `trampsec` section:
//!
//! | symbol      | role                                                |
//! | ----------- | --------------------------------------------------- |
//! | `trampoline`| Page start; provided by `kernel.ld`. Same address  |
//! |             | as `uservec` since uservec is the first label.      |
//! | `uservec`   | Entry from a user-mode trap (`stvec` points here).  |
//! | `userret`   | Returns to user mode after a syscall / interrupt.   |
//!
//! Why `global_asm!` instead of `#[unsafe(naked)]` functions?
//!
//! - We need named labels (`uservec:`, `userret:`) inside the same asm
//!   block. Rust's `naked_asm!` only allows numeric local labels.
//! - We need both labels in the SAME section in a known order.
//! - The kernel's `rust_proc` / `rust_trap` modules declare these as
//!   `extern "C" { static uservec: u8; }`. If we also defined a Rust
//!   `extern "C" fn uservec()` that would clash and rustc would suffix
//!   one of them at link time.
//! - `global_asm!` emits raw assembly at the global level, with no Rust
//!   item bound to the labels — no clash, no name mangling.
//!
//! Field offsets into `Trapframe` are taken from `rust_proc::Trapframe`
//! (`#[repr(C)]`):
//!     0   kernel_satp     104  s1     200  s6      256  t3
//!     8   kernel_sp       112  a0     208  s7      264  t4
//!    16   kernel_trap     120  a1     216  s8      272  t5
//!    24   kernel_pc       128  a2     224  s9      280  t6
//!    32   kernel_hartid   136  a3     232  s10
//!    40   ra              144  a4     240  s11
//!    48   sp              152  a5     248  s11
//!    56   gp              160  a6
//!    64   tp              168  a7
//!    72   t0              176  s2
//!    80   t1              184  s3
//!    88   t2              192  s4
//!    96   s0              200  s5

use core::arch::global_asm;

global_asm!(
    ".section trampsec, \"ax\"",
    ".equ TRAPFRAME, 0x3fffffe000",

    // -------------------------------------------------------------------
    // uservec: entry from user-mode traps. stvec points here while in
    // user mode, so the CPU jumps here in supervisor mode but with the
    // user page table still active (legal because this page is mapped
    // identically in both).
    // -------------------------------------------------------------------
    ".globl uservec",
    "uservec:",
    // Stash user a0 into sscratch so we can use a0 as a scratch reg
    // pointing at the trapframe.
    "csrw sscratch, a0",
    "li a0, TRAPFRAME",

    // Save user registers into the trapframe.
    "sd ra,   40(a0)",
    "sd sp,   48(a0)",
    "sd gp,   56(a0)",
    "sd tp,   64(a0)",
    "sd t0,   72(a0)",
    "sd t1,   80(a0)",
    "sd t2,   88(a0)",
    "sd s0,   96(a0)",
    "sd s1,  104(a0)",
    "sd a1,  120(a0)",
    "sd a2,  128(a0)",
    "sd a3,  136(a0)",
    "sd a4,  144(a0)",
    "sd a5,  152(a0)",
    "sd a6,  160(a0)",
    "sd a7,  168(a0)",
    "sd s2,  176(a0)",
    "sd s3,  184(a0)",
    "sd s4,  192(a0)",
    "sd s5,  200(a0)",
    "sd s6,  208(a0)",
    "sd s7,  216(a0)",
    "sd s8,  224(a0)",
    "sd s9,  232(a0)",
    "sd s10, 240(a0)",
    "sd s11, 248(a0)",
    "sd t3,  256(a0)",
    "sd t4,  264(a0)",
    "sd t5,  272(a0)",
    "sd t6,  280(a0)",

    // Save user a0 (parked in sscratch) into trapframe slot 112.
    "csrr t0, sscratch",
    "sd t0,  112(a0)",

    // Switch to the kernel: load kernel_sp, kernel_hartid, kernel_trap,
    // kernel_satp from the trapframe.
    "ld sp,  8(a0)",
    "ld tp, 32(a0)",
    "ld t0, 16(a0)",
    "ld t1,  0(a0)",

    // Drain memory ops, install kernel page table, flush stale TLB
    // entries.
    "sfence.vma zero, zero",
    "csrw satp, t1",
    "sfence.vma zero, zero",

    // Jump to usertrap() (Rust). Doesn't return.
    "jalr t0",

    // -------------------------------------------------------------------
    // userret: called by usertrap after preparing return state. a0
    // holds the user satp.
    // -------------------------------------------------------------------
    ".globl userret",
    "userret:",
    // Switch back to the user page table.
    "sfence.vma zero, zero",
    "csrw satp, a0",
    "sfence.vma zero, zero",

    "li a0, TRAPFRAME",

    // Restore everything except a0 from the trapframe.
    "ld ra,   40(a0)",
    "ld sp,   48(a0)",
    "ld gp,   56(a0)",
    "ld tp,   64(a0)",
    "ld t0,   72(a0)",
    "ld t1,   80(a0)",
    "ld t2,   88(a0)",
    "ld s0,   96(a0)",
    "ld s1,  104(a0)",
    "ld a1,  120(a0)",
    "ld a2,  128(a0)",
    "ld a3,  136(a0)",
    "ld a4,  144(a0)",
    "ld a5,  152(a0)",
    "ld a6,  160(a0)",
    "ld a7,  168(a0)",
    "ld s2,  176(a0)",
    "ld s3,  184(a0)",
    "ld s4,  192(a0)",
    "ld s5,  200(a0)",
    "ld s6,  208(a0)",
    "ld s7,  216(a0)",
    "ld s8,  224(a0)",
    "ld s9,  232(a0)",
    "ld s10, 240(a0)",
    "ld s11, 248(a0)",
    "ld t3,  256(a0)",
    "ld t4,  264(a0)",
    "ld t5,  272(a0)",
    "ld t6,  280(a0)",

    // Restore user a0 last.
    "ld a0, 112(a0)",

    // sret: returns to user pc / sstatus prepared by usertrapret.
    "sret",

    // Return to the default section so subsequent compiler-emitted code
    // doesn't accidentally land in trampsec.
    ".previous",
);
