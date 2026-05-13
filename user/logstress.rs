#![no_std]

use core::ffi::{c_char, c_int, c_void};

mod rust_user;
use rust_user::*;

const N: c_int = 250;
const SZ: usize = 2000;
static mut BUF: [u8; SZ] = [0; SZ];

#[unsafe(no_mangle)]
pub unsafe extern "C" fn main(argc: c_int, argv: *mut *mut c_char) -> c_int {
    let self_name = argv_at(argv, 0);

    let mut i = 1;
    while i < argc {
        let pid = fork();
        if pid < 0 {
            printf(b"%s: fork failed\n\0".as_ptr().cast(), self_name);
            return 1;
        }
        if pid == 0 {
            let path = argv_at(argv, i);
            let fd = open(path, O_CREATE | O_RDWR);
            if fd < 0 {
                printf(b"%s: create %s failed\n\0".as_ptr().cast(), self_name, path);
                return 1;
            }

            memset(
                core::ptr::addr_of_mut!(BUF).cast::<c_void>(),
                b'0' as c_int + i,
                SZ as u32,
            );

            let mut j = 0;
            while j < N {
                let n = write(fd, core::ptr::addr_of!(BUF).cast::<c_void>(), SZ as c_int);
                if n != SZ as c_int {
                    printf(b"write failed %d\n\0".as_ptr().cast(), n);
                    return 1;
                }
                j += 1;
            }
            return 0;
        }
        i += 1;
    }

    i = 1;
    let mut xstatus = 0;
    while i < argc {
        wait(core::ptr::addr_of_mut!(xstatus));
        if xstatus != 0 {
            return xstatus;
        }
        i += 1;
    }

    0
}
