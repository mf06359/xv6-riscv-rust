use core::ffi::{c_char, c_int, c_uint, c_void};
use core::ptr;

use crate::rust_proc::{myproc, sleep, wakeup};
use crate::rust_spinlock::{acquire, initlock, release, Spinlock};

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Sleeplock {
    pub locked: c_uint,
    pub lk: Spinlock,
    pub name: *mut c_char,
    pub pid: c_int,
}

static SLEEP_LOCK_NAME: &[u8] = b"sleep lock\0";

#[inline(always)]
unsafe fn current_pid() -> c_int {
    let p = myproc();
    if p.is_null() {
        0
    } else {
        (*p).pid
    }
}

#[no_mangle]
pub unsafe extern "C" fn initsleeplock(lk: *mut Sleeplock, name: *mut c_char) {
    initlock(
        ptr::addr_of_mut!((*lk).lk),
        SLEEP_LOCK_NAME.as_ptr().cast::<c_char>() as *mut c_char,
    );
    (*lk).name = name;
    (*lk).locked = 0;
    (*lk).pid = 0;
}

#[no_mangle]
pub unsafe extern "C" fn acquiresleep(lk: *mut Sleeplock) {
    acquire(ptr::addr_of_mut!((*lk).lk));
    while (*lk).locked != 0 {
        sleep(lk.cast::<c_void>(), ptr::addr_of_mut!((*lk).lk));
    }
    (*lk).locked = 1;
    (*lk).pid = current_pid();
    release(ptr::addr_of_mut!((*lk).lk));
}

#[no_mangle]
pub unsafe extern "C" fn releasesleep(lk: *mut Sleeplock) {
    acquire(ptr::addr_of_mut!((*lk).lk));
    (*lk).locked = 0;
    (*lk).pid = 0;
    wakeup(lk.cast::<c_void>());
    release(ptr::addr_of_mut!((*lk).lk));
}

#[no_mangle]
pub unsafe extern "C" fn holdingsleep(lk: *mut Sleeplock) -> c_int {
    acquire(ptr::addr_of_mut!((*lk).lk));
    let r = ((*lk).locked != 0 && (*lk).pid == current_pid()) as c_int;
    release(ptr::addr_of_mut!((*lk).lk));
    r
}
