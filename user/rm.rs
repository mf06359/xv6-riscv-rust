#![no_std]

use core::ffi::{c_char, c_int};

mod rust_user;
use rust_user::*;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn main(argc: c_int, argv: *mut *mut c_char) -> c_int {
    if argc < 2 {
        fprintf(2, b"Usage: rm files...\n\0".as_ptr().cast());
        return 1;
    }

    let mut i = 1;
    while i < argc {
        let path = argv_at(argv, i);
        if unlink(path) < 0 {
            fprintf(2, b"rm: %s failed to delete\n\0".as_ptr().cast(), path);
            return 1;
        }
        i += 1;
    }

    0
}
