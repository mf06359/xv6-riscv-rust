#![no_std]

use core::ffi::{c_char, c_int, c_void};

mod rust_user;
use rust_user::*;

static mut BUF: [u8; 512] = [0; 512];

unsafe fn wc(fd: c_int, name: *const c_char) -> c_int {
    let mut lines = 0;
    let mut words = 0;
    let mut chars = 0;
    let mut inword = 0;

    loop {
        let n = read(fd, core::ptr::addr_of_mut!(BUF).cast::<c_void>(), 512);
        if n <= 0 {
            if n < 0 {
                printf(b"wc: read error\n\0".as_ptr().cast());
                return 1;
            }
            break;
        }

        let mut p = core::ptr::addr_of!(BUF).cast::<u8>();
        let end = p.add(n as usize);
        while p < end {
            let ch = *p as c_char;
            chars += 1;
            if ch == b'\n' as c_char {
                lines += 1;
            }
            if !strchr(b" \r\t\n\x0b\0".as_ptr().cast(), ch).is_null() {
                inword = 0;
            } else if inword == 0 {
                words += 1;
                inword = 1;
            }
            p = p.add(1);
        }
    }

    printf(
        b"%d %d %d %s\n\0".as_ptr().cast(),
        lines,
        words,
        chars,
        name,
    );
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn main(argc: c_int, argv: *mut *mut c_char) -> c_int {
    if argc <= 1 {
        return wc(0, b"\0".as_ptr().cast());
    }

    let mut i = 1;
    while i < argc {
        let path = argv_at(argv, i);
        let fd = open(path, O_RDONLY);
        if fd < 0 {
            printf(b"wc: cannot open %s\n\0".as_ptr().cast(), path);
            return 1;
        }
        let r = wc(fd, path);
        close(fd);
        if r != 0 {
            return r;
        }
        i += 1;
    }

    0
}
