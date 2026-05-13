#![no_std]
#![allow(dead_code)]

// User-space syscall entry stubs. Each function loads the syscall number
// into register a7, executes `ecall` to trap into the kernel, and returns
// whatever the kernel left in a0/a1. Naked so Rust doesn't add any prologue
// or epilogue that would clobber the argument registers a0..a6.

use core::arch::naked_asm;

macro_rules! syscall_stub {
    ($name:ident, $num:expr) => {
        #[unsafe(naked)]
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name() {
            naked_asm!(
                concat!("li a7, ", stringify!($num)),
                "ecall",
                "ret",
            );
        }
    };
}

// Numbers must match kernel/syscall.h.
syscall_stub!(fork,     1);
syscall_stub!(exit,     2);
syscall_stub!(wait,     3);
syscall_stub!(pipe,     4);
syscall_stub!(read,     5);
syscall_stub!(kill,     6);
syscall_stub!(exec,     7);
syscall_stub!(fstat,    8);
syscall_stub!(chdir,    9);
syscall_stub!(dup,     10);
syscall_stub!(getpid,  11);
syscall_stub!(sys_sbrk,12);
syscall_stub!(pause,   13);
syscall_stub!(uptime,  14);
syscall_stub!(open,    15);
syscall_stub!(write,   16);
syscall_stub!(mknod,   17);
syscall_stub!(unlink,  18);
syscall_stub!(link,    19);
syscall_stub!(mkdir,   20);
syscall_stub!(close,   21);
