#![no_std]

use core::ffi::{c_char, c_int, c_void};

mod rust_user;
use rust_user::*;

static mut BUF: [u8; 1024] = [0; 1024];

unsafe fn match_here(re: *mut c_char, text: *mut c_char) -> c_int {
    if *re == 0 {
        return 1;
    }
    if *re.add(1) == b'*' as c_char {
        return match_star(*re, re.add(2), text);
    }
    if *re == b'$' as c_char && *re.add(1) == 0 {
        return (*text == 0) as c_int;
    }
    if *text != 0 && (*re == b'.' as c_char || *re == *text) {
        return match_here(re.add(1), text.add(1));
    }
    0
}

unsafe fn match_star(c: c_char, re: *mut c_char, mut text: *mut c_char) -> c_int {
    loop {
        if match_here(re, text) != 0 {
            return 1;
        }
        if *text == 0 {
            return 0;
        }
        if *text == c || c == b'.' as c_char {
            text = text.add(1);
            continue;
        }
        return 0;
    }
}

unsafe fn match_re(re: *mut c_char, mut text: *mut c_char) -> c_int {
    if *re == b'^' as c_char {
        return match_here(re.add(1), text);
    }
    loop {
        if match_here(re, text) != 0 {
            return 1;
        }
        if *text == 0 {
            return 0;
        }
        text = text.add(1);
    }
}

unsafe fn grep(pattern: *mut c_char, fd: c_int) {
    let mut m: usize = 0;
    loop {
        let n = read(
            fd,
            core::ptr::addr_of_mut!(BUF)
                .cast::<u8>()
                .add(m)
                .cast::<c_void>(),
            (1024usize.wrapping_sub(m).wrapping_sub(1)) as c_int,
        );
        if n <= 0 {
            return;
        }

        m += n as usize;
        *core::ptr::addr_of_mut!(BUF).cast::<u8>().add(m) = 0;

        let base = core::ptr::addr_of_mut!(BUF).cast::<c_char>();
        let mut p = base;
        loop {
            let q = strchr(p, b'\n' as c_char);
            if q.is_null() {
                break;
            }
            *q = 0;
            if match_re(pattern, p) != 0 {
                *q = b'\n' as c_char;
                write(1, p.cast::<c_void>(), q.offset_from(p) as c_int + 1);
            }
            p = q.add(1);
        }

        if m > 0 {
            let consumed = p.offset_from(base) as usize;
            m -= consumed;
            memmove(
                core::ptr::addr_of_mut!(BUF).cast::<c_void>(),
                p.cast::<c_void>(),
                m as c_int,
            );
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn main(argc: c_int, argv: *mut *mut c_char) -> c_int {
    if argc <= 1 {
        fprintf(2, b"usage: grep pattern [file ...]\n\0".as_ptr().cast());
        return 1;
    }
    let pattern = argv_at(argv, 1);

    if argc <= 2 {
        grep(pattern, 0);
        return 0;
    }

    let mut i = 2;
    while i < argc {
        let path = argv_at(argv, i);
        let fd = open(path, O_RDONLY);
        if fd < 0 {
            printf(b"grep: cannot open %s\n\0".as_ptr().cast(), path);
            return 1;
        }
        grep(pattern, fd);
        close(fd);
        i += 1;
    }

    0
}
