use core::ffi::{c_int, c_void};
use core::marker::PhantomData;
use core::ptr;

use crate::rust_bio::{bread, brelse, bpin, bunpin, bwrite, Buf};
use crate::rust_printf::{kprintf2, panic};
use crate::rust_proc::{sleep, wakeup};
use crate::rust_spinlock::{acquire, initlock, release, Spinlock};
use crate::rust_string::memmove;

const MAXOPBLOCKS: c_int = 10;
const LOGBLOCKS: usize = (MAXOPBLOCKS as usize) * 3;
const BSIZE: usize = 1024;

#[repr(C)]
#[derive(Copy, Clone)]
struct Logheader {
    n: c_int,
    block: [c_int; LOGBLOCKS],
}

struct Log {
    lock: Spinlock,
    start: c_int,
    outstanding: c_int,
    committing: c_int,
    dev: c_int,
    lh: Logheader,
}

static mut LOG: Log = Log {
    lock: Spinlock {
        locked: 0,
        name: ptr::null_mut(),
        cpu: ptr::null_mut(),
    },
    start: 0,
    outstanding: 0,
    committing: 0,
    dev: 0,
    lh: Logheader {
        n: 0,
        block: [0; LOGBLOCKS],
    },
};

#[inline(always)]
unsafe fn lh_block(i: c_int) -> *mut c_int {
    ptr::addr_of_mut!(LOG.lh.block).cast::<c_int>().add(i as usize)
}

unsafe fn install_trans(recovering: c_int) {
    let mut tail = 0;
    while tail < LOG.lh.n {
        if recovering != 0 {
            kprintf2(
                b"recovering tail %d dst %d\n\0".as_ptr().cast(),
                tail as u64,
                (*lh_block(tail)) as u64,
            );
        }

        let lbuf = bread(LOG.dev as u32, (LOG.start + tail + 1) as u32);
        let dbuf = bread(LOG.dev as u32, (*lh_block(tail)) as u32);
        memmove(
            ptr::addr_of_mut!((*dbuf).data).cast::<c_void>(),
            ptr::addr_of!((*lbuf).data).cast::<c_void>(),
            BSIZE as u32,
        );
        bwrite(dbuf);
        if recovering == 0 {
            bunpin(dbuf);
        }
        brelse(lbuf);
        brelse(dbuf);

        tail += 1;
    }
}

unsafe fn read_head() {
    let buf = bread(LOG.dev as u32, LOG.start as u32);
    let lh = ptr::addr_of!((*buf).data).cast::<Logheader>();
    LOG.lh.n = (*lh).n;

    let mut i = 0;
    while i < LOG.lh.n {
        *lh_block(i) = ptr::addr_of!((*lh).block).cast::<c_int>().add(i as usize).read();
        i += 1;
    }
    brelse(buf);
}

unsafe fn write_head() {
    let buf = bread(LOG.dev as u32, LOG.start as u32);
    let hb = ptr::addr_of_mut!((*buf).data).cast::<Logheader>();
    (*hb).n = LOG.lh.n;

    let mut i = 0;
    while i < LOG.lh.n {
        ptr::addr_of_mut!((*hb).block)
            .cast::<c_int>()
            .add(i as usize)
            .write(*lh_block(i));
        i += 1;
    }

    bwrite(buf);
    brelse(buf);
}

unsafe fn recover_from_log() {
    read_head();
    install_trans(1);
    LOG.lh.n = 0;
    write_head();
}

#[no_mangle]
pub unsafe extern "C" fn initlog(dev: c_int, sb: *mut crate::rust_fs::Superblock) {
    if core::mem::size_of::<Logheader>() >= BSIZE {
        panic(b"initlog: too big logheader\0".as_ptr().cast_mut().cast());
    }

    initlock(
        ptr::addr_of_mut!(LOG.lock),
        b"log\0".as_ptr().cast_mut().cast(),
    );
    LOG.start = (*sb).logstart as c_int;
    LOG.dev = dev;
    recover_from_log();
}

#[no_mangle]
pub unsafe extern "C" fn begin_op() {
    acquire(ptr::addr_of_mut!(LOG.lock));
    loop {
        if LOG.committing != 0 {
            sleep(
                ptr::addr_of_mut!(LOG).cast::<c_void>(),
                ptr::addr_of_mut!(LOG.lock),
            );
        } else if LOG.lh.n + (LOG.outstanding + 1) * MAXOPBLOCKS > LOGBLOCKS as c_int {
            sleep(
                ptr::addr_of_mut!(LOG).cast::<c_void>(),
                ptr::addr_of_mut!(LOG.lock),
            );
        } else {
            LOG.outstanding += 1;
            release(ptr::addr_of_mut!(LOG.lock));
            break;
        }
    }
}

unsafe fn write_log() {
    let mut tail = 0;
    while tail < LOG.lh.n {
        let to = bread(LOG.dev as u32, (LOG.start + tail + 1) as u32);
        let from = bread(LOG.dev as u32, (*lh_block(tail)) as u32);
        memmove(
            ptr::addr_of_mut!((*to).data).cast::<c_void>(),
            ptr::addr_of!((*from).data).cast::<c_void>(),
            BSIZE as u32,
        );
        bwrite(to);
        brelse(from);
        brelse(to);
        tail += 1;
    }
}

unsafe fn commit() {
    if LOG.lh.n > 0 {
        write_log();
        write_head();
        install_trans(0);
        LOG.lh.n = 0;
        write_head();
    }
}

#[no_mangle]
pub unsafe extern "C" fn end_op() {
    let mut do_commit = 0;

    acquire(ptr::addr_of_mut!(LOG.lock));
    LOG.outstanding -= 1;
    if LOG.committing != 0 {
        panic(b"log.committing\0".as_ptr().cast_mut().cast());
    }
    if LOG.outstanding == 0 {
        do_commit = 1;
        LOG.committing = 1;
    } else {
        wakeup(ptr::addr_of_mut!(LOG).cast::<c_void>());
    }
    release(ptr::addr_of_mut!(LOG.lock));

    if do_commit != 0 {
        commit();
        acquire(ptr::addr_of_mut!(LOG.lock));
        LOG.committing = 0;
        wakeup(ptr::addr_of_mut!(LOG).cast::<c_void>());
        release(ptr::addr_of_mut!(LOG.lock));
    }
}

#[no_mangle]
pub unsafe extern "C" fn log_write(b: *mut Buf) {
    acquire(ptr::addr_of_mut!(LOG.lock));

    if LOG.lh.n >= LOGBLOCKS as c_int {
        panic(b"too big a transaction\0".as_ptr().cast_mut().cast());
    }
    if LOG.outstanding < 1 {
        panic(b"log_write outside of trans\0".as_ptr().cast_mut().cast());
    }

    let mut i = 0;
    while i < LOG.lh.n {
        if *lh_block(i) == (*b).blockno as c_int {
            break;
        }
        i += 1;
    }

    *lh_block(i) = (*b).blockno as c_int;
    if i == LOG.lh.n {
        bpin(b);
        LOG.lh.n += 1;
    }

    release(ptr::addr_of_mut!(LOG.lock));
}

/// RAII wrapper for filesystem log transactions (`begin_op`/`end_op`).
///
/// Acquire with `TxnGuard::begin()`. `end_op()` is guaranteed in `Drop`,
/// so early returns and error branches cannot leak an outstanding
/// transaction.
#[must_use = "transaction stays open until this guard is dropped"]
pub struct TxnGuard {
    active: bool,
    _not_send_sync: PhantomData<*mut ()>,
}

impl TxnGuard {
    #[inline]
    pub fn begin() -> Self {
        unsafe { begin_op() };
        Self {
            active: true,
            _not_send_sync: PhantomData,
        }
    }

    /// Explicitly end the transaction early.
    #[inline]
    pub fn end(mut self) {
        if self.active {
            self.active = false;
            unsafe { end_op() };
        }
    }
}

impl Drop for TxnGuard {
    #[inline]
    fn drop(&mut self) {
        if self.active {
            unsafe { end_op() };
        }
    }
}
