#![no_std]

use core::ffi::{c_char, c_int};

mod rust_user;
use rust_user::*;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn main(argc: c_int, argv: *mut *mut c_char) -> c_int {
    if argc != 3 {
        fprintf(2, b"Usage: ln old new\n\0".as_ptr().cast());
        return 1;
    }

    let old = argv_at(argv, 1);
    let new = argv_at(argv, 2);
    if link(old, new) < 0 {
        fprintf(2, b"link %s %s: failed\n\0".as_ptr().cast(), old, new);
    }
    0
}
