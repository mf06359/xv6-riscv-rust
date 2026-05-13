#![no_std]

use core::ffi::{c_char, c_int};

mod rust_user;
use rust_user::*;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn main(_argc: c_int, argv: *mut *mut c_char) -> c_int {
    let self_name = argv_at(argv, 0);

    if mkdir(b"dd\0".as_ptr().cast()) != 0 {
        printf(b"%s: mkdir dd failed\n\0".as_ptr().cast(), self_name);
        return 1;
    }

    if chdir(b"dd\0".as_ptr().cast()) != 0 {
        printf(b"%s: chdir dd failed\n\0".as_ptr().cast(), self_name);
        return 1;
    }

    if unlink(b"../dd\0".as_ptr().cast()) < 0 {
        printf(b"%s: unlink failed\n\0".as_ptr().cast(), self_name);
        return 1;
    }

    printf(b"wait for kill and reclaim\n\0".as_ptr().cast());
    loop {
        pause(1000);
    }
}
