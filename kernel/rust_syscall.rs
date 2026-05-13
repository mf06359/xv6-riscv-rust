use core::ffi::{c_char, c_int};
use core::mem;
use core::ptr;

use crate::rust_printf::{kprintf3, panic};
use crate::rust_proc::{
    myproc, sys_exit, sys_fork, sys_getpid, sys_kill, sys_pause, sys_sbrk, sys_uptime, sys_wait,
};
use crate::rust_string::strlen;
use crate::rust_sysfile::{
    sys_chdir, sys_close, sys_dup, sys_exec, sys_fstat, sys_link, sys_mkdir, sys_mknod, sys_open,
    sys_pipe, sys_read, sys_unlink, sys_write,
};
use crate::rust_vm::{copyin, copyinstr};

const NSYSCALL: usize = 22;

type SyscallFn = unsafe extern "C" fn() -> u64;

static SYSCALLS: [Option<SyscallFn>; NSYSCALL] = [
    None,
    Some(sys_fork),
    Some(sys_exit),
    Some(sys_wait),
    Some(sys_pipe),
    Some(sys_read),
    Some(sys_kill),
    Some(sys_exec),
    Some(sys_fstat),
    Some(sys_chdir),
    Some(sys_dup),
    Some(sys_getpid),
    Some(sys_sbrk),
    Some(sys_pause),
    Some(sys_uptime),
    Some(sys_open),
    Some(sys_write),
    Some(sys_mknod),
    Some(sys_unlink),
    Some(sys_link),
    Some(sys_mkdir),
    Some(sys_close),
];

unsafe fn argraw(n: c_int) -> u64 {
    let p = myproc();
    let tf = (*p).trapframe;

    match n {
        0 => (*tf).a0,
        1 => (*tf).a1,
        2 => (*tf).a2,
        3 => (*tf).a3,
        4 => (*tf).a4,
        5 => (*tf).a5,
        _ => panic(b"argraw\0".as_ptr().cast_mut().cast()),
    }
}

#[no_mangle]
pub unsafe extern "C" fn fetchaddr(addr: u64, ip: *mut u64) -> c_int {
    let p = myproc();
    let sz = (*p).sz;
    let len = mem::size_of::<u64>() as u64;

    if addr >= sz {
        return -1;
    }
    match addr.checked_add(len) {
        Some(end) if end <= sz => {}
        _ => return -1,
    }

    if copyin((*p).pagetable, ip.cast::<c_char>(), addr, len) != 0 {
        return -1;
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn fetchstr(addr: u64, buf: *mut c_char, max: c_int) -> c_int {
    let p = myproc();
    if copyinstr((*p).pagetable, buf, addr, max as u64) < 0 {
        return -1;
    }
    strlen(buf)
}

#[no_mangle]
pub unsafe extern "C" fn argint(n: c_int, ip: *mut c_int) {
    *ip = argraw(n) as c_int;
}

#[no_mangle]
pub unsafe extern "C" fn argaddr(n: c_int, ip: *mut u64) {
    *ip = argraw(n);
}

#[no_mangle]
pub unsafe extern "C" fn argstr(n: c_int, buf: *mut c_char, max: c_int) -> c_int {
    let mut addr = 0u64;
    argaddr(n, ptr::addr_of_mut!(addr));
    fetchstr(addr, buf, max)
}

#[no_mangle]
pub unsafe extern "C" fn syscall() {
    let p = myproc();
    let tf = (*p).trapframe;
    let num = (*tf).a7 as c_int;

    if num > 0 {
        let idx = num as usize;
        if idx < SYSCALLS.len() {
            if let Some(sys_fn) = SYSCALLS[idx] {
                (*tf).a0 = sys_fn();
                return;
            }
        }
    }

    kprintf3(
        b"%d %s: unknown sys call %d\n\0".as_ptr().cast(),
        (*p).pid as u64,
        ptr::addr_of!((*p).name).cast::<c_char>() as u64,
        num as u64,
    );
    (*tf).a0 = u64::MAX;
}
