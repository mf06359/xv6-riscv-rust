use core::ffi::{c_char, c_int, c_uint, c_void};
use core::ptr;

use crate::rust_file::{filealloc, fileclose, File};
use crate::rust_kalloc::{kalloc, kfree};
use crate::rust_lock::{wake, SpinMutex};
use crate::rust_proc::{killed, myproc, myproc_pagetable};
use crate::rust_vm::{copyin, copyout};

const PIPESIZE: usize = 512;
const FD_PIPE: c_int = 1;

/// Mutable per-pipe state (everything except the lock itself).
struct PipeState {
    data: [u8; PIPESIZE],
    nread: c_uint,
    nwrite: c_uint,
    readopen: c_int,
    writeopen: c_int,
}

/// A pipe is a `SpinMutex<PipeState>` allocated from kalloc. The kernel
/// stores it as `*mut c_void` inside `File::pipe`; the cast back happens
/// only here.
type Pipe = SpinMutex<PipeState>;

#[inline(always)]
unsafe fn as_pipe(pi: *mut c_void) -> &'static Pipe {
    &*(pi as *const Pipe)
}

/// Channel used to wake readers waiting on data.
#[inline(always)]
unsafe fn read_chan(pi: *mut c_void) -> *const c_void {
    let p = as_pipe(pi);
    // SAFETY: only the address is taken; the value is never read.
    ptr::addr_of!((*p.raw_ptr()).cpu) as *const c_void
}

/// Channel used to wake writers waiting for buffer space.
#[inline(always)]
unsafe fn write_chan(pi: *mut c_void) -> *const c_void {
    let p = as_pipe(pi);
    ptr::addr_of!((*p.raw_ptr()).name) as *const c_void
}

#[no_mangle]
pub unsafe extern "C" fn pipealloc(f0: *mut *mut File, f1: *mut *mut File) -> c_int {
    *f0 = ptr::null_mut();
    *f1 = ptr::null_mut();

    *f0 = filealloc();
    if (*f0).is_null() {
        return -1;
    }
    *f1 = filealloc();
    if (*f1).is_null() {
        fileclose(*f0);
        *f0 = ptr::null_mut();
        return -1;
    }

    let raw = kalloc().cast::<Pipe>();
    if raw.is_null() {
        fileclose(*f0);
        fileclose(*f1);
        *f0 = ptr::null_mut();
        *f1 = ptr::null_mut();
        return -1;
    }

    // Construct the SpinMutex in place. `kalloc` gives us an uninitialized
    // page, so we cannot just cast — we must `ptr::write` the new value.
    ptr::write(
        raw,
        SpinMutex::new(
            PipeState {
                data: [0; PIPESIZE],
                nread: 0,
                nwrite: 0,
                readopen: 1,
                writeopen: 1,
            },
            b"pipe\0",
        ),
    );
    (*raw).init();

    let pi_void = raw.cast::<c_void>();

    (**f0).file_type = FD_PIPE;
    (**f0).readable = 1;
    (**f0).writable = 0;
    (**f0).pipe = pi_void;

    (**f1).file_type = FD_PIPE;
    (**f1).readable = 0;
    (**f1).writable = 1;
    (**f1).pipe = pi_void;

    0
}

#[no_mangle]
pub unsafe extern "C" fn pipeclose(pi: *mut c_void, writable: c_int) {
    let pipe = as_pipe(pi);
    let r_chan = read_chan(pi);
    let w_chan = write_chan(pi);
    let should_free;
    {
        let mut g = pipe.lock();
        if writable != 0 {
            g.writeopen = 0;
            wake(r_chan);
        } else {
            g.readopen = 0;
            wake(w_chan);
        }
        should_free = g.readopen == 0 && g.writeopen == 0;
    }
    if should_free {
        // Drop the SpinMutex (no-op for our types) and return the page.
        ptr::drop_in_place(pi.cast::<Pipe>());
        kfree(pi);
    }
}

#[no_mangle]
pub unsafe extern "C" fn pipewrite(pi: *mut c_void, addr: u64, n: c_int) -> c_int {
    let pipe = as_pipe(pi);
    let r_chan = read_chan(pi);
    let w_chan = write_chan(pi);
    let pr = myproc();
    let pagetable = myproc_pagetable();

    let mut i: c_int = 0;
    let mut g = pipe.lock();
    while i < n {
        if g.readopen == 0 || killed(pr) != 0 {
            return -1;
        }
        if g.nwrite == g.nread.wrapping_add(PIPESIZE as c_uint) {
            wake(r_chan);
            g.sleep(w_chan);
        } else {
            let mut ch: c_char = 0;
            if copyin(pagetable, ptr::addr_of_mut!(ch), addr + (i as u64), 1) == -1 {
                break;
            }
            let idx = (g.nwrite % (PIPESIZE as c_uint)) as usize;
            g.data[idx] = ch as u8;
            g.nwrite = g.nwrite.wrapping_add(1);
            i += 1;
        }
    }

    wake(r_chan);
    i
}

#[no_mangle]
pub unsafe extern "C" fn piperead(pi: *mut c_void, addr: u64, n: c_int) -> c_int {
    let pipe = as_pipe(pi);
    let r_chan = read_chan(pi);
    let w_chan = write_chan(pi);
    let pr = myproc();
    let pagetable = myproc_pagetable();

    let mut i: c_int = 0;
    let mut g = pipe.lock();
    while g.nread == g.nwrite && g.writeopen != 0 {
        if killed(pr) != 0 {
            return -1;
        }
        g.sleep(r_chan);
    }

    while i < n {
        if g.nread == g.nwrite {
            break;
        }
        let idx = (g.nread % (PIPESIZE as c_uint)) as usize;
        let mut ch = g.data[idx] as c_char;
        if copyout(pagetable, addr + (i as u64), ptr::addr_of_mut!(ch), 1) == -1 {
            if i == 0 {
                i = -1;
            }
            break;
        }
        g.nread = g.nread.wrapping_add(1);
        i += 1;
    }

    wake(w_chan);
    i
}
