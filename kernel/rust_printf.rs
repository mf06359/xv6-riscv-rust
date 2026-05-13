use core::ffi::{c_char, c_int};
use core::ptr;

use crate::rust_console::consputc;
use crate::rust_spinlock::{acquire, initlock, release, Spinlock};

static DIGITS: &[u8; 16] = b"0123456789abcdef";
static PR_NAME: [u8; 3] = *b"pr\0";

#[no_mangle]
pub static mut panicking: c_int = 0;

#[no_mangle]
pub static mut panicked: c_int = 0;

#[no_mangle]
pub static mut pr_lock: Spinlock = Spinlock {
    locked: 0,
    name: ptr::null_mut(),
    cpu: ptr::null_mut(),
};

#[no_mangle]
pub unsafe extern "C" fn rust_printint(xx: i64, base: c_int, mut sign: c_int) {
    let mut buf = [0u8; 20];
    let mut i = 0usize;
    let mut x: u64;
    if base <= 1 {
        return;
    }
    let base = base as u64;

    if sign != 0 && xx < 0 {
        sign = 1;
        x = xx.wrapping_neg() as u64;
    } else {
        sign = 0;
        x = xx as u64;
    }

    loop {
        let digit_i = (x % base) as usize;
        *buf.as_mut_ptr().add(i) = *DIGITS.as_ptr().add(digit_i);
        i += 1;
        x /= base;
        if x == 0 {
            break;
        }
    }

    if sign != 0 {
        *buf.as_mut_ptr().add(i) = b'-';
        i += 1;
    }

    while i > 0 {
        i -= 1;
        consputc(*buf.as_ptr().add(i) as c_int);
    }
}

#[no_mangle]
pub unsafe extern "C" fn rust_printptr(mut x: u64) {
    consputc(b'0' as c_int);
    consputc(b'x' as c_int);

    let mut i = 0usize;
    while i < 16 {
        let digit_i = (x >> 60) as usize;
        consputc(*DIGITS.as_ptr().add(digit_i) as c_int);
        x <<= 4;
        i += 1;
    }
}

#[inline(always)]
unsafe fn print_bytes(bytes: &[u8]) {
    let mut i = 0usize;
    while i < bytes.len() {
        consputc(bytes[i] as c_int);
        i += 1;
    }
}

#[inline(always)]
unsafe fn print_cstr(mut s: *const c_char) {
    if s.is_null() {
        print_bytes(b"(null)");
        return;
    }
    while *s != 0 {
        consputc(*s as c_int);
        s = s.add(1);
    }
}

#[inline(always)]
unsafe fn next_arg(argp: *const u64, ai: &mut usize) -> u64 {
    let out = if *ai < 16 {
        ptr::read(argp.add(*ai))
    } else {
        0
    };
    *ai += 1;
    out
}

#[no_mangle]
pub unsafe extern "C" fn printf(
    fmt: *mut c_char,
    a1: u64,
    a2: u64,
    a3: u64,
    a4: u64,
    a5: u64,
    a6: u64,
    a7: u64,
    a8: u64,
    a9: u64,
    a10: u64,
    a11: u64,
    a12: u64,
    a13: u64,
    a14: u64,
    a15: u64,
    a16: u64,
) -> c_int {
    let args = [
        a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15, a16,
    ];
    let argp = args.as_ptr();
    let mut ai = 0usize;

    if panicking == 0 {
        acquire(ptr::addr_of_mut!(pr_lock));
    }

    let mut i: isize = 0;
    loop {
        let cx = *fmt.offset(i) as u8;
        if cx == 0 {
            break;
        }
        if cx != b'%' {
            consputc(cx as c_int);
            i += 1;
            continue;
        }

        i += 1;
        let c0 = *fmt.offset(i) as u8;
        let mut c1 = 0u8;
        let mut c2 = 0u8;
        if c0 != 0 {
            c1 = *fmt.offset(i + 1) as u8;
        }
        if c1 != 0 {
            c2 = *fmt.offset(i + 2) as u8;
        }

        if c0 == b'd' {
            rust_printint(next_arg(argp, &mut ai) as i32 as i64, 10, 1);
        } else if c0 == b'l' && c1 == b'd' {
            rust_printint(next_arg(argp, &mut ai) as i64, 10, 1);
            i += 1;
        } else if c0 == b'l' && c1 == b'l' && c2 == b'd' {
            rust_printint(next_arg(argp, &mut ai) as i64, 10, 1);
            i += 2;
        } else if c0 == b'u' {
            rust_printint(next_arg(argp, &mut ai) as u32 as i64, 10, 0);
        } else if c0 == b'l' && c1 == b'u' {
            rust_printint(next_arg(argp, &mut ai) as i64, 10, 0);
            i += 1;
        } else if c0 == b'l' && c1 == b'l' && c2 == b'u' {
            rust_printint(next_arg(argp, &mut ai) as i64, 10, 0);
            i += 2;
        } else if c0 == b'x' {
            rust_printint(next_arg(argp, &mut ai) as u32 as i64, 16, 0);
        } else if c0 == b'l' && c1 == b'x' {
            rust_printint(next_arg(argp, &mut ai) as i64, 16, 0);
            i += 1;
        } else if c0 == b'l' && c1 == b'l' && c2 == b'x' {
            rust_printint(next_arg(argp, &mut ai) as i64, 16, 0);
            i += 2;
        } else if c0 == b'p' {
            rust_printptr(next_arg(argp, &mut ai));
        } else if c0 == b'c' {
            consputc(next_arg(argp, &mut ai) as c_int);
        } else if c0 == b's' {
            let mut s = next_arg(argp, &mut ai) as *const c_char;
            if s.is_null() {
                s = b"(null)\0".as_ptr().cast();
            }
            print_cstr(s);
        } else if c0 == b'%' {
            consputc(b'%' as c_int);
        } else if c0 == 0 {
            break;
        } else {
            consputc(b'%' as c_int);
            consputc(c0 as c_int);
        }

        i += 1;
    }

    if panicking == 0 {
        release(ptr::addr_of_mut!(pr_lock));
    }

    0
}

#[inline(always)]
pub unsafe fn kprintf0(fmt: *const c_char) -> c_int {
    printf(fmt as *mut c_char, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)
}

#[inline(always)]
pub unsafe fn kprintf1(fmt: *const c_char, a1: u64) -> c_int {
    printf(fmt as *mut c_char, a1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)
}

#[inline(always)]
pub unsafe fn kprintf2(fmt: *const c_char, a1: u64, a2: u64) -> c_int {
    printf(fmt as *mut c_char, a1, a2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)
}

#[inline(always)]
pub unsafe fn kprintf3(fmt: *const c_char, a1: u64, a2: u64, a3: u64) -> c_int {
    printf(fmt as *mut c_char, a1, a2, a3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)
}

#[no_mangle]
pub unsafe extern "C" fn panic(s: *mut c_char) -> ! {
    panicking = 1;
    print_bytes(b"panic: ");
    print_cstr(s);
    consputc(b'\n' as c_int);
    panicked = 1;
    loop {
        core::hint::spin_loop();
    }
}

#[no_mangle]
pub unsafe extern "C" fn printfinit() {
    initlock(
        ptr::addr_of_mut!(pr_lock),
        PR_NAME.as_ptr().cast_mut().cast(),
    );
}
