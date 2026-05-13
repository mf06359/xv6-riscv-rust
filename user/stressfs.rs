#![no_std]

use core::ffi::{c_char, c_int, c_void};

mod rust_user;
use rust_user::*;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn main(_argc: c_int, _argv: *mut *mut c_char) -> c_int {
    let mut path: [u8; 10] = *b"stressfs0\0";
    let mut data: [u8; 512] = [b'a'; 512];

    printf(b"stressfs starting\n\0".as_ptr().cast());

    let mut i = 0;
    while i < 4 {
        if fork() > 0 {
            break;
        }
        i += 1;
    }

    printf(b"write %d\n\0".as_ptr().cast(), i);
    let path_ch = path.as_mut_ptr().add(8);
    *path_ch = (*path_ch).wrapping_add(i as u8);

    let fd = open(path.as_ptr().cast(), O_CREATE | O_RDWR);
    let mut j = 0;
    while j < 20 {
        write(fd, data.as_ptr().cast::<c_void>(), 512);
        j += 1;
    }
    close(fd);

    printf(b"read\n\0".as_ptr().cast());

    let fd2 = open(path.as_ptr().cast(), O_RDONLY);
    j = 0;
    while j < 20 {
        read(fd2, data.as_mut_ptr().cast::<c_void>(), 512);
        j += 1;
    }
    close(fd2);

    wait(core::ptr::null_mut());
    0
}
