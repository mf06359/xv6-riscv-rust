#![no_std]

use core::ffi::{c_char, c_int};

mod rust_user;
use rust_user::*;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn main(_argc: c_int, argv: *mut *mut c_char) -> c_int {
    let self_name = argv_at(argv, 0);
    let ff = b"file0\0".as_ptr().cast::<c_char>();

    let fd = open(ff, O_CREATE | O_WRONLY);
    if fd < 0 {
        printf(b"%s: open failed\n\0".as_ptr().cast(), self_name);
        return 1;
    }

    let mut st = Stat {
        dev: 0,
        ino: 0,
        file_type: 0,
        nlink: 0,
        size: 0,
    };
    if fstat(fd, core::ptr::addr_of_mut!(st)) < 0 {
        fprintf(
            2,
            b"%s: cannot stat %s\n\0".as_ptr().cast(),
            self_name,
            b"ff\0".as_ptr().cast::<c_char>(),
        );
        return 1;
    }

    if unlink(ff) < 0 {
        printf(b"%s: unlink failed\n\0".as_ptr().cast(), self_name);
        return 1;
    }

    if open(ff, O_RDONLY) != -1 {
        printf(b"%s: open successed\n\0".as_ptr().cast(), self_name);
        return 1;
    }

    printf(
        b"wait for kill and reclaim %d\n\0".as_ptr().cast(),
        st.ino as c_int,
    );
    loop {
        pause(1000);
    }
}
