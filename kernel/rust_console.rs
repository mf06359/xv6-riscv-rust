use core::ffi::{c_int, c_void};
use core::ptr;

use crate::rust_file::devsw;
use crate::rust_lock::{wake, SpinMutex};
use crate::rust_proc::{either_copyin, either_copyout, killed, myproc, procdump};
use crate::rust_uart::{uartinit, uartputc_sync, uartwrite};

const BACKSPACE: c_int = 0x100;
const INPUT_BUF_SIZE: usize = 128;
const CONSOLE: usize = 1;

#[inline(always)]
const fn ctrl(x: u8) -> c_int {
    (x as c_int) - ('@' as c_int)
}

/// Console input ring buffer state. The lock that guards this struct is
/// supplied by the surrounding `SpinMutex`.
struct ConsState {
    buf: [u8; INPUT_BUF_SIZE],
    /// Read index — bytes consumed by `consoleread`.
    r: u32,
    /// Write index — bytes the line discipline has committed (visible to
    /// readers).
    w: u32,
    /// Edit index — newest byte typed (may not yet be at a line boundary).
    e: u32,
}

static CONS: SpinMutex<ConsState> = SpinMutex::new(
    ConsState {
        buf: [0; INPUT_BUF_SIZE],
        r: 0,
        w: 0,
        e: 0,
    },
    b"cons\0",
);

#[no_mangle]
pub unsafe extern "C" fn consputc(c: c_int) {
    if c == BACKSPACE {
        uartputc_sync('\x08' as c_int);
        uartputc_sync(' ' as c_int);
        uartputc_sync('\x08' as c_int);
    } else {
        uartputc_sync(c);
    }
}

unsafe extern "C" fn consolewrite(user_src: c_int, src: u64, n: c_int) -> c_int {
    let mut buf = [0u8; 32];
    let mut i: c_int = 0;

    while i < n {
        let mut nn = buf.len() as c_int;
        if nn > n - i {
            nn = n - i;
        }
        if either_copyin(
            buf.as_mut_ptr().cast::<c_void>(),
            user_src,
            src + i as u64,
            nn as u64,
        ) == -1
        {
            break;
        }
        uartwrite(buf.as_mut_ptr().cast(), nn);
        i += nn;
    }

    i
}

unsafe extern "C" fn consoleread(user_dst: c_int, mut dst: u64, mut n: c_int) -> c_int {
    let target = n;
    let chan = CONS.chan();
    let mut cons = CONS.lock();

    while n > 0 {
        // Wait for at least one whole line to be queued.
        while cons.r == cons.w {
            if killed(myproc()) != 0 {
                return -1;
            }
            cons.sleep(chan);
        }

        let c = cons.buf[(cons.r as usize) % INPUT_BUF_SIZE] as c_int;
        cons.r = cons.r.wrapping_add(1);

        if c == ctrl(b'D') {
            // Push back ^D so the next read sees EOF.
            if n < target {
                cons.r = cons.r.wrapping_sub(1);
            }
            break;
        }

        let mut cbuf = c as u8;
        if either_copyout(user_dst, dst, ptr::addr_of_mut!(cbuf).cast(), 1) == -1 {
            break;
        }

        dst = dst.wrapping_add(1);
        n -= 1;

        if c == ('\n' as c_int) {
            break;
        }
    }

    target - n
}

#[no_mangle]
pub unsafe extern "C" fn consoleintr(mut c: c_int) {
    let chan = CONS.chan();
    let mut cons = CONS.lock();

    match c {
        x if x == ctrl(b'P') => {
            procdump();
        }
        x if x == ctrl(b'U') => {
            // Erase the current line.
            while cons.e != cons.w
                && cons.buf[((cons.e.wrapping_sub(1)) as usize) % INPUT_BUF_SIZE] != b'\n'
            {
                cons.e = cons.e.wrapping_sub(1);
                consputc(BACKSPACE);
            }
        }
        x if x == ctrl(b'H') || x == 0x7f => {
            if cons.e != cons.w {
                cons.e = cons.e.wrapping_sub(1);
                consputc(BACKSPACE);
            }
        }
        _ => {
            if c != 0 && cons.e.wrapping_sub(cons.r) < INPUT_BUF_SIZE as u32 {
                if c == ('\r' as c_int) {
                    c = '\n' as c_int;
                }
                consputc(c);
                let idx = (cons.e as usize) % INPUT_BUF_SIZE;
                cons.buf[idx] = c as u8;
                cons.e = cons.e.wrapping_add(1);

                if c == ('\n' as c_int)
                    || c == ctrl(b'D')
                    || cons.e.wrapping_sub(cons.r) == INPUT_BUF_SIZE as u32
                {
                    // Commit the line and wake the reader.
                    cons.w = cons.e;
                    wake(chan);
                }
            }
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn consoleinit() {
    CONS.init();
    uartinit();

    devsw[CONSOLE].read = Some(consoleread);
    devsw[CONSOLE].write = Some(consolewrite);
}
