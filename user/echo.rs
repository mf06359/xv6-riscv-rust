#![no_std]

use core::ffi::{c_char, c_int, c_void};

mod rust_user;
use rust_user::*;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn main(argc: c_int, argv: *mut *mut c_char) -> c_int {
    let mut i = 1;
    while i < argc {
        let arg = argv_at(argv, i);
        write(1, arg.cast::<c_void>(), strlen(arg) as c_int);
        if i + 1 < argc {
            write(1, b" ".as_ptr().cast(), 1);
        } else {
            write(1, b"\n".as_ptr().cast(), 1);
        }
        i += 1;
    }
    0
}
