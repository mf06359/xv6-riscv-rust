#![no_std]

use core::ffi::{c_char, c_int};

mod rust_user;
use rust_user::*;

static mut ARGV: [*mut c_char; 2] = [b"sh\0".as_ptr() as *mut c_char, core::ptr::null_mut()];

#[unsafe(no_mangle)]
pub unsafe extern "C" fn main(_argc: c_int, _argv: *mut *mut c_char) -> c_int {
    if open(b"console\0".as_ptr().cast(), O_RDWR) < 0 {
        mknod(b"console\0".as_ptr().cast(), CONSOLE, 0);
        open(b"console\0".as_ptr().cast(), O_RDWR);
    }
    dup(0);
    dup(0);

    loop {
        printf(b"init: starting sh\n\0".as_ptr().cast());
        let pid = fork();
        if pid < 0 {
            printf(b"init: fork failed\n\0".as_ptr().cast());
            return 1;
        }
        if pid == 0 {
            exec(b"sh\0".as_ptr().cast(), core::ptr::addr_of_mut!(ARGV).cast::<*mut c_char>());
            printf(b"init: exec sh failed\n\0".as_ptr().cast());
            return 1;
        }

        loop {
            let wpid = wait(core::ptr::null_mut());
            if wpid == pid {
                break;
            } else if wpid < 0 {
                printf(b"init: wait returned an error\n\0".as_ptr().cast());
                return 1;
            }
        }
    }
}
