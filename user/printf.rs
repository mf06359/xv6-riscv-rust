#![no_std]

use core::ffi::{c_char, c_int, c_void};

unsafe extern "C" {
    fn write(fd: c_int, buf: *const c_void, n: c_int) -> c_int;
}

static DIGITS: &[u8] = b"0123456789ABCDEF";

const PRINTBUF: usize = 256;

/// Single-threaded user-side print buffer. Coalesces single-byte writes
/// from `printf`/`fprintf` into one `write(2)` syscall per ~256 bytes
/// (or per call). The kernel UART path is the expensive part — every
/// `write` we save is a syscall avoided.
struct PrintBuf {
    buf: [u8; PRINTBUF],
    len: usize,
    fd: c_int,
}

static mut PRINT_BUF: PrintBuf = PrintBuf {
    buf: [0; PRINTBUF],
    len: 0,
    fd: -1,
};

#[inline(always)]
unsafe fn flush_locked() {
    if PRINT_BUF.len > 0 {
        write(
            PRINT_BUF.fd,
            (&raw const PRINT_BUF.buf).cast::<c_void>(),
            PRINT_BUF.len as c_int,
        );
        PRINT_BUF.len = 0;
    }
}

unsafe fn putc(fd: c_int, c: u8) {
    // Switching fds: flush whatever we had buffered first.
    if PRINT_BUF.fd != fd {
        flush_locked();
        PRINT_BUF.fd = fd;
    }
    let i = PRINT_BUF.len;
    (&raw mut PRINT_BUF.buf).cast::<u8>().add(i).write(c);
    PRINT_BUF.len = i + 1;
    if PRINT_BUF.len == PRINTBUF {
        flush_locked();
    }
}

unsafe fn printint(fd: c_int, xx: i64, base: u32, sgn: bool) {
    let mut buf = [0u8; 20];
    let neg = sgn && xx < 0;
    let mut x: u64 = if neg { (-xx) as u64 } else { xx as u64 };

    let mut i = 0usize;
    loop {
        *buf.get_unchecked_mut(i) = *DIGITS.get_unchecked((x % base as u64) as usize);
        i += 1;
        x /= base as u64;
        if x == 0 {
            break;
        }
    }
    if neg {
        *buf.get_unchecked_mut(i) = b'-';
        i += 1;
    }
    while i > 0 {
        i -= 1;
        putc(fd, *buf.get_unchecked(i));
    }
}

unsafe fn printptr(fd: c_int, mut x: u64) {
    putc(fd, b'0');
    putc(fd, b'x');
    let mut i = 0;
    while i < (core::mem::size_of::<u64>() * 2) {
        let nibble = (x >> (core::mem::size_of::<u64>() * 8 - 4)) as usize;
        putc(fd, *DIGITS.get_unchecked(nibble));
        x <<= 4;
        i += 1;
    }
}

struct Args {
    a: [u64; 7],
    idx: usize,
}

impl Args {
    unsafe fn next(&mut self) -> u64 {
        let v = *self.a.get_unchecked(self.idx);
        self.idx += 1;
        v
    }
}

unsafe fn vprintf_impl(fd: c_int, fmt: *const c_char, mut args: Args) {
    let mut i: usize = 0;
    let mut state: u8 = 0;

    loop {
        let c0_raw = *fmt.add(i);
        if c0_raw == 0 {
            break;
        }
        let c0 = c0_raw as u8;

        if state == 0 {
            if c0 == b'%' {
                state = b'%';
            } else {
                putc(fd, c0);
            }
        } else if state == b'%' {
            let c1_raw = *fmt.add(i + 1);
            let c1 = c1_raw as u8;
            let c2 = if c1 != 0 { *fmt.add(i + 2) as u8 } else { 0 };

            if c0 == b'd' {
                printint(fd, args.next() as i32 as i64, 10, true);
            } else if c0 == b'l' && c1 == b'd' {
                printint(fd, args.next() as i64, 10, true);
                i += 1;
            } else if c0 == b'l' && c1 == b'l' && c2 == b'd' {
                printint(fd, args.next() as i64, 10, true);
                i += 2;
            } else if c0 == b'u' {
                printint(fd, args.next() as u32 as i64, 10, false);
            } else if c0 == b'l' && c1 == b'u' {
                printint(fd, args.next() as i64, 10, false);
                i += 1;
            } else if c0 == b'l' && c1 == b'l' && c2 == b'u' {
                printint(fd, args.next() as i64, 10, false);
                i += 2;
            } else if c0 == b'x' {
                printint(fd, args.next() as u32 as i64, 16, false);
            } else if c0 == b'l' && c1 == b'x' {
                printint(fd, args.next() as i64, 16, false);
                i += 1;
            } else if c0 == b'l' && c1 == b'l' && c2 == b'x' {
                printint(fd, args.next() as i64, 16, false);
                i += 2;
            } else if c0 == b'p' {
                printptr(fd, args.next());
            } else if c0 == b'c' {
                putc(fd, args.next() as u8);
            } else if c0 == b's' {
                let s = args.next() as *const c_char;
                if s.is_null() {
                    let null_str = b"(null)";
                    let mut k = 0;
                    while k < null_str.len() {
                        putc(fd, *null_str.get_unchecked(k));
                        k += 1;
                    }
                } else {
                    let mut p = s;
                    while *p != 0 {
                        putc(fd, *p as u8);
                        p = p.add(1);
                    }
                }
            } else if c0 == b'%' {
                putc(fd, b'%');
            } else {
                putc(fd, b'%');
                putc(fd, c0);
            }

            state = 0;
        }

        i += 1;
    }
    // Caller may rely on output being visible (e.g. shell prompt without
    // newline), so flush at end of every printf/fprintf call.
    flush_locked();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn fprintf(
    fd: c_int,
    fmt: *const c_char,
    a1: u64,
    a2: u64,
    a3: u64,
    a4: u64,
    a5: u64,
    a6: u64,
    a7: u64,
) {
    let args = Args {
        a: [a1, a2, a3, a4, a5, a6, a7],
        idx: 0,
    };
    vprintf_impl(fd, fmt, args);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn printf(
    fmt: *const c_char,
    a1: u64,
    a2: u64,
    a3: u64,
    a4: u64,
    a5: u64,
    a6: u64,
    a7: u64,
) {
    let args = Args {
        a: [a1, a2, a3, a4, a5, a6, a7],
        idx: 0,
    };
    vprintf_impl(1, fmt, args);
}

