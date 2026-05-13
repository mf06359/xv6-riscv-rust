use core::ffi::{c_int, c_uint};
use core::ptr;

use crate::rust_printf::panic;
use crate::rust_sleeplock::{acquiresleep, holdingsleep, initsleeplock, releasesleep, Sleeplock};
use crate::rust_spinlock::{acquire, initlock, release, Spinlock};
use crate::rust_virtio_disk::virtio_disk_rw;

const BSIZE: usize = 1024;
const NBUF: usize = 30;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Buf {
    pub valid: c_int,
    pub disk: c_int,
    pub dev: c_uint,
    pub blockno: c_uint,
    pub lock: Sleeplock,
    pub refcnt: c_uint,
    pub prev: *mut Buf,
    pub next: *mut Buf,
    pub data: [u8; BSIZE],
}

struct Bcache {
    lock: Spinlock,
    buf: [Buf; NBUF],
    head: Buf,
}

const EMPTY_SPINLOCK: Spinlock = Spinlock {
    locked: 0,
    name: ptr::null_mut(),
    cpu: ptr::null_mut(),
};

const EMPTY_SLEEPLOCK: Sleeplock = Sleeplock {
    locked: 0,
    lk: EMPTY_SPINLOCK,
    name: ptr::null_mut(),
    pid: 0,
};

const EMPTY_BUF: Buf = Buf {
    valid: 0,
    disk: 0,
    dev: 0,
    blockno: 0,
    lock: EMPTY_SLEEPLOCK,
    refcnt: 0,
    prev: ptr::null_mut(),
    next: ptr::null_mut(),
    data: [0; BSIZE],
};

static mut BCACHE: Bcache = Bcache {
    lock: EMPTY_SPINLOCK,
    buf: [EMPTY_BUF; NBUF],
    head: EMPTY_BUF,
};

unsafe fn bget(dev: c_uint, blockno: c_uint) -> *mut Buf {
    acquire(ptr::addr_of_mut!(BCACHE.lock));

    let mut b = ptr::addr_of_mut!(BCACHE.head).cast::<Buf>();
    b = (*b).next;
    while b != ptr::addr_of_mut!(BCACHE.head) {
        if (*b).dev == dev && (*b).blockno == blockno {
            (*b).refcnt = (*b).refcnt.wrapping_add(1);
            release(ptr::addr_of_mut!(BCACHE.lock));
            acquiresleep(ptr::addr_of_mut!((*b).lock));
            return b;
        }
        b = (*b).next;
    }

    b = (*ptr::addr_of_mut!(BCACHE.head)).prev;
    while b != ptr::addr_of_mut!(BCACHE.head) {
        if (*b).refcnt == 0 {
            (*b).dev = dev;
            (*b).blockno = blockno;
            (*b).valid = 0;
            (*b).refcnt = 1;
            release(ptr::addr_of_mut!(BCACHE.lock));
            acquiresleep(ptr::addr_of_mut!((*b).lock));
            return b;
        }
        b = (*b).prev;
    }

    panic(b"bget: no buffers\0".as_ptr().cast_mut().cast())
}

#[no_mangle]
pub unsafe extern "C" fn binit() {
    initlock(
        ptr::addr_of_mut!(BCACHE.lock),
        b"bcache\0".as_ptr().cast_mut().cast(),
    );

    (*ptr::addr_of_mut!(BCACHE.head)).prev = ptr::addr_of_mut!(BCACHE.head);
    (*ptr::addr_of_mut!(BCACHE.head)).next = ptr::addr_of_mut!(BCACHE.head);

    let mut i = 0usize;
    while i < NBUF {
        let b = ptr::addr_of_mut!(BCACHE.buf).cast::<Buf>().add(i);
        (*b).next = (*ptr::addr_of_mut!(BCACHE.head)).next;
        (*b).prev = ptr::addr_of_mut!(BCACHE.head);
        initsleeplock(
            ptr::addr_of_mut!((*b).lock),
            b"buffer\0".as_ptr().cast_mut().cast(),
        );
        (*(*ptr::addr_of_mut!(BCACHE.head)).next).prev = b;
        (*ptr::addr_of_mut!(BCACHE.head)).next = b;
        i += 1;
    }
}

#[no_mangle]
pub unsafe extern "C" fn bread(dev: c_uint, blockno: c_uint) -> *mut Buf {
    let b = bget(dev, blockno);
    if (*b).valid == 0 {
        virtio_disk_rw(b, 0);
        (*b).valid = 1;
    }
    b
}

#[no_mangle]
pub unsafe extern "C" fn bwrite(b: *mut Buf) {
    if holdingsleep(ptr::addr_of_mut!((*b).lock)) == 0 {
        panic(b"bwrite\0".as_ptr().cast_mut().cast());
    }
    virtio_disk_rw(b, 1);
}

#[no_mangle]
pub unsafe extern "C" fn brelse(b: *mut Buf) {
    if holdingsleep(ptr::addr_of_mut!((*b).lock)) == 0 {
        panic(b"brelse\0".as_ptr().cast_mut().cast());
    }

    releasesleep(ptr::addr_of_mut!((*b).lock));

    acquire(ptr::addr_of_mut!(BCACHE.lock));
    (*b).refcnt = (*b).refcnt.wrapping_sub(1);
    if (*b).refcnt == 0 {
        (*(*b).next).prev = (*b).prev;
        (*(*b).prev).next = (*b).next;
        (*b).next = (*ptr::addr_of_mut!(BCACHE.head)).next;
        (*b).prev = ptr::addr_of_mut!(BCACHE.head);
        (*(*ptr::addr_of_mut!(BCACHE.head)).next).prev = b;
        (*ptr::addr_of_mut!(BCACHE.head)).next = b;
    }
    release(ptr::addr_of_mut!(BCACHE.lock));
}

#[no_mangle]
pub unsafe extern "C" fn bpin(b: *mut Buf) {
    acquire(ptr::addr_of_mut!(BCACHE.lock));
    (*b).refcnt = (*b).refcnt.wrapping_add(1);
    release(ptr::addr_of_mut!(BCACHE.lock));
}

#[no_mangle]
pub unsafe extern "C" fn bunpin(b: *mut Buf) {
    acquire(ptr::addr_of_mut!(BCACHE.lock));
    (*b).refcnt = (*b).refcnt.wrapping_sub(1);
    release(ptr::addr_of_mut!(BCACHE.lock));
}
