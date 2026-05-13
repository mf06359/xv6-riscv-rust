use core::ffi::{c_char, c_int, c_uint};
use core::marker::PhantomData;
use core::ptr;
use core::sync::atomic::{fence, AtomicU32, Ordering};

use crate::rust_printf::panic;
use crate::rust_proc::mycpu;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Context {
    pub ra: u64,
    pub sp: u64,
    pub s0: u64,
    pub s1: u64,
    pub s2: u64,
    pub s3: u64,
    pub s4: u64,
    pub s5: u64,
    pub s6: u64,
    pub s7: u64,
    pub s8: u64,
    pub s9: u64,
    pub s10: u64,
    pub s11: u64,
}

#[repr(C)]
pub struct Cpu {
    pub proc: *mut core::ffi::c_void,
    pub context: Context,
    pub noff: c_int,
    pub intena: c_int,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Spinlock {
    pub locked: c_uint,
    pub name: *mut c_char,
    pub cpu: *mut Cpu,
}

const SSTATUS_SIE: u64 = 1 << 1;

#[inline(always)]
unsafe fn intr_get() -> c_int {
    let mut x: u64;
    core::arch::asm!("csrr {0}, sstatus", out(reg) x);
    ((x & SSTATUS_SIE) != 0) as c_int
}

#[inline(always)]
pub unsafe fn intr_on() {
    let mut x: u64;
    core::arch::asm!("csrr {0}, sstatus", out(reg) x);
    x |= SSTATUS_SIE;
    core::arch::asm!("csrw sstatus, {0}", in(reg) x);
}

#[inline(always)]
pub unsafe fn intr_off() {
    let mut x: u64;
    core::arch::asm!("csrr {0}, sstatus", out(reg) x);
    x &= !SSTATUS_SIE;
    core::arch::asm!("csrw sstatus, {0}", in(reg) x);
}

#[inline(always)]
unsafe fn panic_with(msg: &'static [u8]) -> ! {
    panic(msg.as_ptr().cast_mut().cast())
}

#[inline(always)]
unsafe fn locked_atomic(lk: *mut Spinlock) -> *const AtomicU32 {
    ptr::addr_of_mut!((*lk).locked).cast::<AtomicU32>()
}

#[no_mangle]
pub unsafe extern "C" fn initlock(lk: *mut Spinlock, name: *mut c_char) {
    (*lk).name = name;
    (*lk).locked = 0;
    (*lk).cpu = ptr::null_mut();
}

#[no_mangle]
pub unsafe extern "C" fn acquire(lk: *mut Spinlock) {
    push_off();
    if holding(lk) != 0 {
        panic_with(b"acquire\0");
    }

    let atomic = &*locked_atomic(lk);
    while atomic.swap(1, Ordering::Acquire) != 0 {}
    fence(Ordering::SeqCst);

    (*lk).cpu = mycpu();
}

#[no_mangle]
pub unsafe extern "C" fn release(lk: *mut Spinlock) {
    if holding(lk) == 0 {
        panic_with(b"release\0");
    }

    (*lk).cpu = ptr::null_mut();
    fence(Ordering::SeqCst);

    let atomic = &*locked_atomic(lk);
    atomic.store(0, Ordering::Release);

    pop_off();
}

#[no_mangle]
pub unsafe extern "C" fn holding(lk: *mut Spinlock) -> c_int {
    ((*lk).locked != 0 && (*lk).cpu == mycpu()) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn push_off() {
    let old = intr_get();
    intr_off();
    let c = mycpu();
    if (*c).noff == 0 {
        (*c).intena = old;
    }
    (*c).noff += 1;
}

#[no_mangle]
pub unsafe extern "C" fn pop_off() {
    let c = mycpu();
    if intr_get() != 0 {
        panic_with(b"pop_off - interruptible\0");
    }
    if (*c).noff < 1 {
        panic_with(b"pop_off\0");
    }
    (*c).noff -= 1;
    if (*c).noff == 0 && (*c).intena != 0 {
        intr_on();
    }
}

/// RAII helper for `push_off`/`pop_off`.
///
/// Constructing this guard disables interrupts for the current CPU context
/// (via nested `push_off` semantics). Dropping it restores the previous
/// interrupt state with `pop_off`.
#[must_use = "interrupts stay disabled until this guard is dropped"]
pub struct InterruptGuard {
    // Raw pointers are !Send + !Sync. Keep the guard thread-local by type.
    _not_send_sync: PhantomData<*mut ()>,
}

impl InterruptGuard {
    #[inline(always)]
    pub fn new() -> Self {
        unsafe { push_off() };
        Self {
            _not_send_sync: PhantomData,
        }
    }
}

impl Drop for InterruptGuard {
    #[inline(always)]
    fn drop(&mut self) {
        unsafe { pop_off() };
    }
}
