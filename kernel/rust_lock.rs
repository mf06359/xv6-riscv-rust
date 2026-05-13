//! Safe locking abstractions wrapping the raw spinlock and sleeplock primitives.
//!
//! These types localize `unsafe` to a small set of methods (`lock()`, the
//! `Drop` impls on guards, and the `sleep()` shim that talks to `rust_proc`)
//! so the rest of the kernel can use locked data through normal Rust
//! references obtained from RAII guards.
//!
//! # Quick reference
//!
//! ```ignore
//! // 1. Spin lock guarding shared data:
//! static FOO: SpinMutex<Foo> = SpinMutex::new(Foo::new(), b"foo\0");
//! {
//!     let mut g = FOO.lock();
//!     g.field = 42;
//! } // released automatically
//!
//! // 2. Wait on a condition variable while holding the lock
//! //    (atomic release-and-sleep, lock reacquired before returning):
//! let mut g = FOO.lock();
//! while !g.ready {
//!     g.sleep(FOO.chan());
//! }
//! wake(FOO.chan()); // anywhere; does not require the lock
//!
//! // 3. Sleep lock for long-held mutual exclusion:
//! let inode_lock: SleepMutex<Inode> = SleepMutex::new(Inode::default(), b"inode\0");
//! let mut g = inode_lock.lock();   // may block; releases CPU while waiting
//! g.bump();
//! ```
//!
//! New modules should prefer `SpinMutex<T>` / `SleepMutex<T>` over the raw
//! `acquire` / `release` / `acquiresleep` / `releasesleep` primitives.
#![allow(dead_code)]

use core::cell::UnsafeCell;
use core::ffi::{c_char, c_void};
use core::ops::{Deref, DerefMut};

use crate::rust_proc::{sleep as proc_sleep, wakeup as proc_wakeup};
use crate::rust_sleeplock::{acquiresleep, holdingsleep, initsleeplock, releasesleep, Sleeplock};
use crate::rust_spinlock::{acquire, initlock, release, Spinlock};

// ============================================================================
// SpinMutex<T>
// ============================================================================

/// A spin lock guarding a value of type `T`.
///
/// `Sync` is implemented manually because `UnsafeCell<T>` is not `Sync`,
/// but the spinlock makes interior mutation safe across threads.
pub struct SpinMutex<T> {
    raw: UnsafeCell<Spinlock>,
    data: UnsafeCell<T>,
    name: *const u8,
    init_done: UnsafeCell<bool>,
}

unsafe impl<T: Send> Sync for SpinMutex<T> {}
unsafe impl<T: Send> Send for SpinMutex<T> {}

impl<T> SpinMutex<T> {
    /// Create a new `SpinMutex`. `name` must be a NUL-terminated byte string
    /// usable as a static debug name.
    pub const fn new(value: T, name: &'static [u8]) -> Self {
        Self {
            raw: UnsafeCell::new(Spinlock {
                locked: 0,
                name: core::ptr::null_mut(),
                cpu: core::ptr::null_mut(),
            }),
            data: UnsafeCell::new(value),
            name: name.as_ptr(),
            init_done: UnsafeCell::new(false),
        }
    }

    /// Initialize the underlying raw spinlock (idempotent).
    /// `lock()` will call this automatically on first use, but boot code may
    /// call it explicitly to make ordering obvious.
    pub fn init(&self) {
        unsafe {
            if !*self.init_done.get() {
                initlock(self.raw.get(), self.name as *mut c_char);
                *self.init_done.get() = true;
            }
        }
    }

    /// Acquire the lock, returning a guard. The guard derefs to `&T` /
    /// `&mut T`. The lock is released when the guard is dropped.
    pub fn lock(&self) -> SpinGuard<'_, T> {
        unsafe {
            if !*self.init_done.get() {
                self.init();
            }
            acquire(self.raw.get());
        }
        SpinGuard { mutex: self }
    }

    /// Pointer to the inner `Spinlock`. Useful when calling out to legacy C
    /// or assembly code that expects a `*mut Spinlock`. Most callers should
    /// use `lock()` instead.
    #[inline]
    pub fn raw_ptr(&self) -> *mut Spinlock {
        self.raw.get()
    }

    /// A stable address inside the mutex usable as a sleep/wakeup channel.
    /// Two calls on the same mutex always return the same pointer; different
    /// mutexes return different pointers, so no aliasing.
    #[inline]
    pub fn chan(&self) -> *const c_void {
        self.raw.get() as *const c_void
    }
}

/// RAII guard returned by `SpinMutex::lock()`.
pub struct SpinGuard<'a, T> {
    mutex: &'a SpinMutex<T>,
}

impl<T> SpinGuard<'_, T> {
    /// Atomically release the spin lock, sleep on `chan` until woken via
    /// `wake(chan)`, and reacquire the lock before returning.
    ///
    /// Mirrors xv6's `sleep(chan, &lk)` C API but keeps the lock invariant
    /// expressed through the guard: the caller still holds the guard after
    /// `sleep` returns, just like a normal critical section.
    pub fn sleep(&mut self, chan: *const c_void) {
        unsafe {
            proc_sleep(chan as *mut c_void, self.mutex.raw.get());
        }
    }

    /// Run `cond` repeatedly; sleep on `chan` whenever it returns `true`.
    /// Idiomatic for "wait until predicate becomes false" patterns.
    pub fn wait_while<F: FnMut(&T) -> bool>(&mut self, chan: *const c_void, mut cond: F) {
        while cond(&**self) {
            self.sleep(chan);
        }
    }
}

impl<T> Deref for SpinGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.mutex.data.get() }
    }
}

impl<T> DerefMut for SpinGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<T> Drop for SpinGuard<'_, T> {
    fn drop(&mut self) {
        unsafe { release(self.mutex.raw.get()) }
    }
}

// ============================================================================
// SleepMutex<T>
// ============================================================================

/// A sleep lock guarding a value of type `T`. Prefer this over `SpinMutex`
/// for long-held critical sections (e.g. inode I/O), where holding a spin
/// lock would be wasteful or incorrect (cannot block while spinning).
pub struct SleepMutex<T> {
    raw: UnsafeCell<Sleeplock>,
    data: UnsafeCell<T>,
    name: *const u8,
    init_done: UnsafeCell<bool>,
}

unsafe impl<T: Send> Sync for SleepMutex<T> {}
unsafe impl<T: Send> Send for SleepMutex<T> {}

impl<T> SleepMutex<T> {
    pub const fn new(value: T, name: &'static [u8]) -> Self {
        Self {
            raw: UnsafeCell::new(Sleeplock {
                locked: 0,
                lk: Spinlock {
                    locked: 0,
                    name: core::ptr::null_mut(),
                    cpu: core::ptr::null_mut(),
                },
                name: core::ptr::null_mut(),
                pid: 0,
            }),
            data: UnsafeCell::new(value),
            name: name.as_ptr(),
            init_done: UnsafeCell::new(false),
        }
    }

    pub fn init(&self) {
        unsafe {
            if !*self.init_done.get() {
                initsleeplock(self.raw.get(), self.name as *mut c_char);
                *self.init_done.get() = true;
            }
        }
    }

    /// Acquire the sleep lock; may block (yields CPU) until available.
    pub fn lock(&self) -> SleepGuard<'_, T> {
        unsafe {
            if !*self.init_done.get() {
                self.init();
            }
            acquiresleep(self.raw.get());
        }
        SleepGuard { mutex: self }
    }

    /// True if the *current* process holds this sleep lock.
    pub fn held_by_me(&self) -> bool {
        unsafe { holdingsleep(self.raw.get()) != 0 }
    }
}

pub struct SleepGuard<'a, T> {
    mutex: &'a SleepMutex<T>,
}

impl<T> Deref for SleepGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.mutex.data.get() }
    }
}

impl<T> DerefMut for SleepGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<T> Drop for SleepGuard<'_, T> {
    fn drop(&mut self) {
        unsafe { releasesleep(self.mutex.raw.get()) }
    }
}

// ============================================================================
// Free helpers
// ============================================================================

/// Wake every process sleeping on `chan`. Symmetric counterpart to
/// `SpinGuard::sleep`.
#[inline]
pub fn wake(chan: *const c_void) {
    unsafe { proc_wakeup(chan as *mut c_void) }
}
