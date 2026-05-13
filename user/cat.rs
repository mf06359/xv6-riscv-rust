#![no_std]

use core::ffi::{c_char, c_int, c_void};

mod rust_user;
use rust_user::*;

static mut BUF: [u8; 512] = [0; 512];

unsafe fn cat(fd: c_int) -> c_int {
    loop {
        let n = read(fd, core::ptr::addr_of_mut!(BUF).cast::<c_void>(), 512);
        if n > 0 {
            if write(1, core::ptr::addr_of!(BUF).cast::<c_void>(), n) != n {
                fprintf(2, b"cat: write error\n\0".as_ptr().cast());
                return 1;
            }
            continue;
        }
        if n < 0 {
            fprintf(2, b"cat: read error\n\0".as_ptr().cast());
            return 1;
        }
        return 0;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn main(argc: c_int, argv: *mut *mut c_char) -> c_int {
    if argc <= 1 {
        return cat(0);
    }

    let mut i = 1;
    while i < argc {
        let path = argv_at(argv, i);
        let fd = open(path, O_RDONLY);
        if fd < 0 {
            fprintf(2, b"cat: cannot open %s\n\0".as_ptr().cast(), path);
            return 1;
        }
        let r = cat(fd);
        close(fd);
        if r != 0 {
            return r;
        }
        i += 1;
    }

    0
}
