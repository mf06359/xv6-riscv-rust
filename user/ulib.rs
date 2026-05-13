#![no_std]

use core::ffi::{c_char, c_int, c_uint, c_void};

const O_RDONLY: c_int = 0;
const SBRK_EAGER: c_int = 1;
const SBRK_LAZY: c_int = 2;

#[repr(C)]
pub struct Stat {
    dev: c_int,
    ino: c_uint,
    file_type: i16,
    nlink: i16,
    size: u64,
}

unsafe extern "C" {
    fn main(argc: c_int, argv: *mut *mut c_char) -> c_int;
    fn exit(status: c_int) -> !;
    fn read(fd: c_int, buf: *mut c_void, n: c_int) -> c_int;
    fn open(path: *const c_char, mode: c_int) -> c_int;
    fn close(fd: c_int) -> c_int;
    fn fstat(fd: c_int, st: *mut Stat) -> c_int;
    fn sys_sbrk(n: c_int, kind: c_int) -> *mut c_char;
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn start(argc: c_int, argv: *mut *mut c_char) {
    let r = main(argc, argv);
    exit(r);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strcpy(mut s: *mut c_char, mut t: *const c_char) -> *mut c_char {
    let os = s;
    loop {
        *s = *t;
        if *s == 0 {
            break;
        }
        s = s.add(1);
        t = t.add(1);
    }
    os
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strcmp(mut p: *const c_char, mut q: *const c_char) -> c_int {
    while *p != 0 && *p == *q {
        p = p.add(1);
        q = q.add(1);
    }
    (*p as u8 as c_int) - (*q as u8 as c_int)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strlen(mut s: *const c_char) -> c_uint {
    let mut n: c_uint = 0;
    while *s != 0 {
        n = n.wrapping_add(1);
        s = s.add(1);
    }
    n
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memset(dst: *mut c_void, c: c_int, n: c_uint) -> *mut c_void {
    let mut p = dst as *mut u8;
    let mut i = 0;
    while i < n {
        *p = c as u8;
        p = p.add(1);
        i += 1;
    }
    dst
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strchr(mut s: *const c_char, c: c_char) -> *mut c_char {
    while *s != 0 {
        if *s == c {
            return s as *mut c_char;
        }
        s = s.add(1);
    }
    core::ptr::null_mut()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gets(buf: *mut c_char, max: c_int) -> *mut c_char {
    let mut i = 0;
    while i + 1 < max {
        let mut c: c_char = 0;
        let cc = read(0, (&mut c as *mut c_char).cast::<c_void>(), 1);
        if cc < 1 {
            break;
        }
        *buf.add(i as usize) = c;
        i += 1;
        if c == b'\n' as c_char || c == b'\r' as c_char {
            break;
        }
    }
    *buf.add(i as usize) = 0;
    buf
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn stat(n: *const c_char, st: *mut Stat) -> c_int {
    let fd = open(n, O_RDONLY);
    if fd < 0 {
        return -1;
    }
    let r = fstat(fd, st);
    close(fd);
    r
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn atoi(mut s: *const c_char) -> c_int {
    let mut n = 0;
    while *s >= b'0' as c_char && *s <= b'9' as c_char {
        n = n * 10 + (*s as c_int - b'0' as c_int);
        s = s.add(1);
    }
    n
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memmove(
    vdst: *mut c_void,
    vsrc: *const c_void,
    n: c_int,
) -> *mut c_void {
    let mut dst = vdst as *mut u8;
    let mut src = vsrc as *const u8;
    if (src as usize) > (dst as usize) {
        let mut i = 0;
        while i < n {
            *dst = *src;
            dst = dst.add(1);
            src = src.add(1);
            i += 1;
        }
    } else {
        dst = dst.add(n as usize);
        src = src.add(n as usize);
        let mut i = 0;
        while i < n {
            dst = dst.sub(1);
            src = src.sub(1);
            *dst = *src;
            i += 1;
        }
    }
    vdst
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcmp(s1: *const c_void, s2: *const c_void, n: c_uint) -> c_int {
    let mut p1 = s1 as *const u8;
    let mut p2 = s2 as *const u8;
    let mut i = 0;
    while i < n {
        if *p1 != *p2 {
            return (*p1 as c_int) - (*p2 as c_int);
        }
        p1 = p1.add(1);
        p2 = p2.add(1);
        i += 1;
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcpy(dst: *mut c_void, src: *const c_void, n: c_uint) -> *mut c_void {
    memmove(dst, src, n as c_int)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sbrk(n: c_int) -> *mut c_char {
    sys_sbrk(n, SBRK_EAGER)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sbrklazy(n: c_int) -> *mut c_char {
    sys_sbrk(n, SBRK_LAZY)
}
