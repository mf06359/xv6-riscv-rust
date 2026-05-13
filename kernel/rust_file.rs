use core::ffi::{c_char, c_int, c_short, c_uint, c_void};
use core::mem::size_of;
use core::ptr;

use crate::rust_fs::{iput, readi, stati, writei, Inode, InodeGuard, Stat};
use crate::rust_log::TxnGuard;
use crate::rust_pipe::{pipeclose, piperead, pipewrite};
use crate::rust_printf::panic;
use crate::rust_proc::myproc_pagetable;
use crate::rust_spinlock::{acquire, initlock, release, Spinlock};
use crate::rust_vm::copyout;

const NFILE: usize = 100;
const NDEV: usize = 10;
const FD_NONE: c_int = 0;
const FD_PIPE: c_int = 1;
const FD_INODE: c_int = 2;
const FD_DEVICE: c_int = 3;
const MAXOPBLOCKS: c_int = 10;
const BSIZE: c_int = 1024;

type DevReadFn = unsafe extern "C" fn(c_int, u64, c_int) -> c_int;
type DevWriteFn = unsafe extern "C" fn(c_int, u64, c_int) -> c_int;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct File {
    pub file_type: c_int,
    pub r#ref: c_int,
    pub readable: c_char,
    pub writable: c_char,
    pub pipe: *mut c_void,
    pub ip: *mut c_void,
    pub off: c_uint,
    pub major: c_short,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Devsw {
    pub read: Option<DevReadFn>,
    pub write: Option<DevWriteFn>,
}

struct FTable {
    lock: Spinlock,
    file: [File; NFILE],
}

const EMPTY_FILE: File = File {
    file_type: FD_NONE,
    r#ref: 0,
    readable: 0,
    writable: 0,
    pipe: ptr::null_mut(),
    ip: ptr::null_mut(),
    off: 0,
    major: 0,
};

const EMPTY_DEVSW: Devsw = Devsw {
    read: None,
    write: None,
};

static mut FTABLE: FTable = FTable {
    lock: Spinlock {
        locked: 0,
        name: ptr::null_mut(),
        cpu: ptr::null_mut(),
    },
    file: [EMPTY_FILE; NFILE],
};

#[no_mangle]
pub static mut devsw: [Devsw; NDEV] = [EMPTY_DEVSW; NDEV];

static FTABLE_NAME: &[u8] = b"ftable\0";
static FILEDUP_PANIC: &[u8] = b"filedup\0";
static FILECLOSE_PANIC: &[u8] = b"fileclose\0";
static FILEREAD_PANIC: &[u8] = b"fileread\0";
static FILEWRITE_PANIC: &[u8] = b"filewrite\0";

#[no_mangle]
pub unsafe extern "C" fn fileinit() {
    initlock(
        ptr::addr_of_mut!(FTABLE.lock),
        FTABLE_NAME.as_ptr().cast::<c_char>() as *mut c_char,
    );
}

#[no_mangle]
pub unsafe extern "C" fn filealloc() -> *mut File {
    acquire(ptr::addr_of_mut!(FTABLE.lock));
    let mut i = 0usize;
    while i < NFILE {
        let f = ptr::addr_of_mut!(FTABLE.file).cast::<File>().add(i);
        if (*f).r#ref == 0 {
            (*f).r#ref = 1;
            release(ptr::addr_of_mut!(FTABLE.lock));
            return f;
        }
        i += 1;
    }
    release(ptr::addr_of_mut!(FTABLE.lock));
    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn filedup(f: *mut File) -> *mut File {
    acquire(ptr::addr_of_mut!(FTABLE.lock));
    if (*f).r#ref < 1 {
        panic(FILEDUP_PANIC.as_ptr().cast::<c_char>() as *mut c_char);
    }
    (*f).r#ref += 1;
    release(ptr::addr_of_mut!(FTABLE.lock));
    f
}

#[no_mangle]
pub unsafe extern "C" fn fileclose(f: *mut File) {
    let ff: File;

    acquire(ptr::addr_of_mut!(FTABLE.lock));
    if (*f).r#ref < 1 {
        panic(FILECLOSE_PANIC.as_ptr().cast::<c_char>() as *mut c_char);
    }
    (*f).r#ref -= 1;
    if (*f).r#ref > 0 {
        release(ptr::addr_of_mut!(FTABLE.lock));
        return;
    }
    ff = *f;
    (*f).r#ref = 0;
    (*f).file_type = FD_NONE;
    release(ptr::addr_of_mut!(FTABLE.lock));

    if ff.file_type == FD_PIPE {
        pipeclose(ff.pipe, ff.writable as c_int);
    } else if ff.file_type == FD_INODE || ff.file_type == FD_DEVICE {
        let _tx = TxnGuard::begin();
        iput(ff.ip.cast::<Inode>());
    }
}

#[no_mangle]
pub unsafe extern "C" fn filestat(f: *mut File, addr: u64) -> c_int {
    if (*f).file_type == FD_INODE || (*f).file_type == FD_DEVICE {
        let mut st = Stat {
            dev: 0,
            ino: 0,
            inode_type: 0,
            nlink: 0,
            size: 0,
        };
        let ip = InodeGuard::lock((*f).ip.cast::<Inode>());
        stati(ip.as_ptr(), ptr::addr_of_mut!(st));
        let pagetable = myproc_pagetable();
        if copyout(
            pagetable,
            addr,
            ptr::addr_of_mut!(st).cast::<c_char>(),
            size_of::<Stat>() as u64,
        ) < 0
        {
            return -1;
        }
        return 0;
    }
    -1
}

#[no_mangle]
pub unsafe extern "C" fn fileread(f: *mut File, addr: u64, n: c_int) -> c_int {
    if (*f).readable == 0 {
        return -1;
    }

    if (*f).file_type == FD_PIPE {
        piperead((*f).pipe, addr, n)
    } else if (*f).file_type == FD_DEVICE {
        let major = (*f).major as c_int;
        if major < 0 || major >= NDEV as c_int {
            return -1;
        }
        let readf = devsw[major as usize].read;
        let Some(readf) = readf else {
            return -1;
        };
        readf(1, addr, n)
    } else if (*f).file_type == FD_INODE {
        let ip = InodeGuard::lock((*f).ip.cast::<Inode>());
        let r = readi(ip.as_ptr(), 1, addr, (*f).off, n as c_uint);
        if r > 0 {
            (*f).off = (*f).off.wrapping_add(r as c_uint);
        }
        r
    } else {
        panic(FILEREAD_PANIC.as_ptr().cast::<c_char>() as *mut c_char);
    }
}

#[no_mangle]
pub unsafe extern "C" fn filewrite(f: *mut File, addr: u64, n: c_int) -> c_int {
    if (*f).writable == 0 {
        return -1;
    }

    if (*f).file_type == FD_PIPE {
        pipewrite((*f).pipe, addr, n)
    } else if (*f).file_type == FD_DEVICE {
        let major = (*f).major as c_int;
        if major < 0 || major >= NDEV as c_int {
            return -1;
        }
        let writef = devsw[major as usize].write;
        let Some(writef) = writef else {
            return -1;
        };
        writef(1, addr, n)
    } else if (*f).file_type == FD_INODE {
        let max = ((MAXOPBLOCKS - 1 - 1 - 2) / 2) * BSIZE;
        let mut i: c_int = 0;
        while i < n {
            let r: c_int;
            let mut n1 = n - i;
            if n1 > max {
                n1 = max;
            }

            let _tx = TxnGuard::begin();
            let ip = InodeGuard::lock((*f).ip.cast::<Inode>());
            r = writei(ip.as_ptr(), 1, addr + (i as u64), (*f).off, n1 as c_uint);
            if r > 0 {
                (*f).off = (*f).off.wrapping_add(r as c_uint);
            }

            if r != n1 {
                break;
            }
            i += r;
        }
        if i == n { n } else { -1 }
    } else {
        panic(FILEWRITE_PANIC.as_ptr().cast::<c_char>() as *mut c_char);
    }
}
