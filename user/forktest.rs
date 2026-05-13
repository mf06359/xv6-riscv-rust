#![no_std]

use core::ffi::{c_char, c_int, c_void};

mod rust_user;
use rust_user::*;

const N: c_int = 1000;

unsafe fn print(msg: *const c_char) {
    write(1, msg.cast::<c_void>(), strlen(msg) as c_int);
}

unsafe fn forktest() -> c_int {
    print(b"fork test\n\0".as_ptr().cast());

    let mut n = 0;
    while n < N {
        let pid = fork();
        if pid < 0 {
            break;
        }
        if pid == 0 {
            return 0;
        }
        n += 1;
    }

    if n == N {
        print(b"fork claimed to work N times!\n\0".as_ptr().cast());
        return 1;
    }

    while n > 0 {
        if wait(core::ptr::null_mut()) < 0 {
            print(b"wait stopped early\n\0".as_ptr().cast());
            return 1;
        }
        n -= 1;
    }

    if wait(core::ptr::null_mut()) != -1 {
        print(b"wait got too many\n\0".as_ptr().cast());
        return 1;
    }

    print(b"fork test OK\n\0".as_ptr().cast());
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn main(_argc: c_int, _argv: *mut *mut c_char) -> c_int {
    forktest()
}
