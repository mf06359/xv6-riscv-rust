#![allow(dead_code)]

use core::ffi::{c_char, c_int, c_short, c_uint, c_void};
use core::panic::PanicInfo;

pub const O_RDONLY: c_int = 0x000;
pub const O_WRONLY: c_int = 0x001;
pub const O_RDWR: c_int = 0x002;
pub const O_CREATE: c_int = 0x200;
pub const O_TRUNC: c_int = 0x400;

pub const T_DIR: c_short = 1;
pub const T_FILE: c_short = 2;
pub const T_DEVICE: c_short = 3;

pub const CONSOLE: c_short = 1;
pub const DIRSIZ: usize = 14;

// kernel/param.h
pub const MAXARG: usize = 32;
pub const MAXOPBLOCKS: usize = 10;
pub const MAXPATH: usize = 128;
pub const NINODE: usize = 50;
pub const USERSTACK: u64 = 1;

// kernel/fs.h
pub const BSIZE: usize = 1024;
pub const NDIRECT: usize = 12;
pub const NINDIRECT: usize = BSIZE / 4;
pub const MAXFILE: usize = NDIRECT + NINDIRECT;

// kernel/riscv.h
pub const PGSIZE: u64 = 4096;
pub const MAXVA: u64 = 1u64 << (9 + 9 + 9 + 12 - 1);

// kernel/memlayout.h
pub const KERNBASE: u64 = 0x80000000;
pub const TRAMPOLINE: u64 = MAXVA - PGSIZE;
pub const TRAPFRAME: u64 = TRAMPOLINE - PGSIZE;

// user.h
pub const SBRK_ERROR: *mut c_char = !0usize as *mut c_char;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Stat {
    pub dev: c_int,
    pub ino: c_uint,
    pub file_type: c_short,
    pub nlink: c_short,
    pub size: u64,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Dirent {
    pub inum: u16,
    pub name: [c_char; DIRSIZ],
}

unsafe extern "C" {
    pub fn fork() -> c_int;
    pub fn exit(status: c_int) -> !;
    pub fn wait(status: *mut c_int) -> c_int;
    pub fn pipe(fd: *mut c_int) -> c_int;
    pub fn write(fd: c_int, buf: *const c_void, n: c_int) -> c_int;
    pub fn read(fd: c_int, buf: *mut c_void, n: c_int) -> c_int;
    pub fn close(fd: c_int) -> c_int;
    pub fn kill(pid: c_int) -> c_int;
    pub fn exec(path: *const c_char, argv: *mut *mut c_char) -> c_int;
    pub fn open(path: *const c_char, flags: c_int) -> c_int;
    pub fn mknod(path: *const c_char, major: c_short, minor: c_short) -> c_int;
    pub fn unlink(path: *const c_char) -> c_int;
    pub fn fstat(fd: c_int, st: *mut Stat) -> c_int;
    pub fn link(old: *const c_char, new: *const c_char) -> c_int;
    pub fn mkdir(path: *const c_char) -> c_int;
    pub fn chdir(path: *const c_char) -> c_int;
    pub fn dup(fd: c_int) -> c_int;
    pub fn getpid() -> c_int;
    pub fn pause(ticks: c_int) -> c_int;
    pub fn uptime() -> c_int;

    pub fn stat(path: *const c_char, st: *mut Stat) -> c_int;
    pub fn strcpy(dst: *mut c_char, src: *const c_char) -> *mut c_char;
    pub fn memmove(dst: *mut c_void, src: *const c_void, n: c_int) -> *mut c_void;
    pub fn strchr(s: *const c_char, c: c_char) -> *mut c_char;
    pub fn strcmp(a: *const c_char, b: *const c_char) -> c_int;
    pub fn strlen(s: *const c_char) -> c_uint;
    pub fn memset(dst: *mut c_void, c: c_int, n: c_uint) -> *mut c_void;
    pub fn atoi(s: *const c_char) -> c_int;
    pub fn memcmp(a: *const c_void, b: *const c_void, n: c_uint) -> c_int;
    pub fn memcpy(dst: *mut c_void, src: *const c_void, n: c_uint) -> *mut c_void;

    pub fn fprintf(fd: c_int, fmt: *const c_char, ...);
    pub fn printf(fmt: *const c_char, ...);

    pub fn sbrk(n: c_int) -> *mut c_char;
    pub fn sbrklazy(n: c_int) -> *mut c_char;
    pub fn malloc(n: c_uint) -> *mut c_void;
    pub fn free(p: *mut c_void);
    pub fn gets(buf: *mut c_char, max: c_int) -> *mut c_char;
}

#[inline(always)]
pub unsafe fn argv_at(argv: *mut *mut c_char, idx: c_int) -> *mut c_char {
    *argv.add(idx as usize)
}

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    unsafe {
        let msg = b"user panic\n";
        write(2, msg.as_ptr().cast::<c_void>(), msg.len() as c_int);
        exit(1);
    }
}
