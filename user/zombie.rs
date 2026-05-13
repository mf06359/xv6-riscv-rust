#![no_std]

use core::ffi::c_int;

mod rust_user;
use rust_user::*;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn main(_argc: c_int, _argv: *mut *mut i8) -> c_int {
    if fork() > 0 {
        pause(5);
    }
    0
}
