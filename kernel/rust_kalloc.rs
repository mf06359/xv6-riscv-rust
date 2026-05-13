use core::ffi::{c_char, c_uint, c_void};
use core::ptr;
use core::sync::atomic::{AtomicU16, Ordering};

use crate::rust_printf::panic;
use crate::rust_spinlock::{acquire, initlock, release, Spinlock};
use crate::rust_string::memset;

extern "C" {
    static end: c_char;
}

#[repr(C)]
struct Run {
    next: *mut Run,
}

#[repr(C)]
struct Kmem {
    lock: Spinlock,
    freelist: *mut Run,
}

const PGSIZE: usize = 4096;
const KERNBASE: usize = 0x8000_0000;
const PHYSTOP: usize = KERNBASE + 128 * 1024 * 1024;

/// One refcount per physical page in the [KERNBASE, PHYSTOP) range.
/// Pages occupied by the kernel image (before `end`) keep refcount 0
/// since they are never owned by `kalloc` callers; the COW page-fault
/// handler also never sees them. Indexing by `(pa - KERNBASE) / PGSIZE`
/// trades a tiny amount of unused entries for a single shift+sub.
const NPAGES: usize = (PHYSTOP - KERNBASE) / PGSIZE;

#[repr(transparent)]
struct PageRef(AtomicU16);

impl PageRef {
    const fn new() -> Self {
        Self(AtomicU16::new(0))
    }
}

static REFCNT: [PageRef; NPAGES] = {
    // Build a const array of zero-initialized AtomicU16. Cannot use
    // [PageRef::new(); NPAGES] because PageRef isn't `Copy`.
    const Z: PageRef = PageRef::new();
    [Z; NPAGES]
};

#[inline(always)]
fn pa_to_idx(pa: usize) -> usize {
    (pa - KERNBASE) / PGSIZE
}

#[inline(always)]
unsafe fn refcnt_at(pa: usize) -> &'static AtomicU16 {
    &REFCNT.get_unchecked(pa_to_idx(pa)).0
}

/// Atomically set a page's refcount to 1. Used right after `kalloc`
/// hands out a freshly recycled page.
#[inline]
unsafe fn refcnt_set_one(pa: usize) {
    refcnt_at(pa).store(1, Ordering::Release);
}

/// Atomically increment a page's refcount. Used to share a page across
/// processes (COW fork).
#[no_mangle]
pub unsafe extern "C" fn kref_inc(pa: *mut c_void) {
    let pa = pa as usize;
    if pa < KERNBASE || pa >= PHYSTOP || (pa % PGSIZE) != 0 {
        panic_with(b"kref_inc\0");
    }
    let prev = refcnt_at(pa).fetch_add(1, Ordering::AcqRel);
    if prev == 0 {
        panic_with(b"kref_inc on free page\0");
    }
}

/// Atomically decrement a page's refcount. Returns the new count.
#[inline]
unsafe fn refcnt_dec(pa: usize) -> u16 {
    let prev = refcnt_at(pa).fetch_sub(1, Ordering::AcqRel);
    if prev == 0 {
        panic_with(b"refcnt underflow\0");
    }
    prev - 1
}

/// Read the current refcount. Useful for diagnostic / debug paths.
#[no_mangle]
pub unsafe extern "C" fn kref_get(pa: *mut c_void) -> u16 {
    let pa = pa as usize;
    if pa < KERNBASE || pa >= PHYSTOP {
        return 0;
    }
    refcnt_at(pa).load(Ordering::Acquire)
}

static mut KMEM: Kmem = Kmem {
    lock: Spinlock {
        locked: 0,
        name: ptr::null_mut(),
        cpu: ptr::null_mut(),
    },
    freelist: ptr::null_mut(),
};

static KMEM_NAME: [u8; 5] = *b"kmem\0";

#[inline(always)]
fn pgroundup(addr: usize) -> usize {
    (addr + PGSIZE - 1) & !(PGSIZE - 1)
}

#[inline(always)]
unsafe fn panic_with(msg: &'static [u8]) -> ! {
    panic(msg.as_ptr().cast_mut().cast())
}

/// Free pages [pa_start, pa_end) into the kalloc pool. Boot-time only —
/// each page goes in with refcount 0 (`kfree` semantics: a refcount-1
/// page being released).
unsafe fn freerange(pa_start: *mut c_void, pa_end: *mut c_void) {
    let mut p = pgroundup(pa_start as usize);
    let pa_end = pa_end as usize;
    while p + PGSIZE <= pa_end {
        // Force refcount to 1 so the kfree() below decrements it to 0
        // and links the page into the freelist.
        refcnt_at(p).store(1, Ordering::Release);
        kfree(p as *mut c_void);
        p += PGSIZE;
    }
}

#[no_mangle]
pub unsafe extern "C" fn kinit() {
    initlock(
        ptr::addr_of_mut!(KMEM.lock),
        KMEM_NAME.as_ptr().cast_mut().cast(),
    );
    freerange(ptr::addr_of!(end).cast_mut().cast(), PHYSTOP as *mut c_void);
}

#[no_mangle]
pub unsafe extern "C" fn kfree(pa: *mut c_void) {
    let pa = pa as usize;
    let end_addr = ptr::addr_of!(end) as usize;
    if (pa % PGSIZE) != 0 || pa < end_addr || pa >= PHYSTOP {
        panic_with(b"kfree\0");
    }

    // Decrement refcount; only the last reference actually returns the
    // page to the freelist. This is what makes COW page sharing safe.
    if refcnt_dec(pa) != 0 {
        return;
    }

    memset(pa as *mut c_void, 1, PGSIZE as c_uint);

    let r = pa as *mut Run;
    let kmem = ptr::addr_of_mut!(KMEM);

    acquire(ptr::addr_of_mut!((*kmem).lock));
    (*r).next = (*kmem).freelist;
    (*kmem).freelist = r;
    release(ptr::addr_of_mut!((*kmem).lock));
}

#[no_mangle]
pub unsafe extern "C" fn kalloc() -> *mut c_void {
    let kmem = ptr::addr_of_mut!(KMEM);

    acquire(ptr::addr_of_mut!((*kmem).lock));
    let r = (*kmem).freelist;
    if !r.is_null() {
        (*kmem).freelist = (*r).next;
    }
    release(ptr::addr_of_mut!((*kmem).lock));

    if !r.is_null() {
        // The page is now logically owned by the caller — refcount 1.
        refcnt_set_one(r as usize);
        memset(r.cast(), 5, PGSIZE as c_uint);
    }

    r.cast()
}

/// Owned handle for one page allocated by `kalloc`.
///
/// The page is automatically released with `kfree` on drop. This lets
/// callers write allocation code that is failure-safe with normal Rust
/// control flow.
#[must_use = "allocated pages must be kept alive or explicitly leaked"]
pub struct KallocPage {
    pa: *mut c_void,
}

impl KallocPage {
    /// Allocate one physical page from kalloc.
    #[inline]
    pub fn alloc() -> Option<Self> {
        let pa = unsafe { kalloc() };
        if pa.is_null() {
            None
        } else {
            Some(Self { pa })
        }
    }

    #[inline]
    pub fn as_ptr(&self) -> *mut c_void {
        self.pa
    }

    /// Transfer ownership back to raw form without freeing on drop.
    #[inline]
    pub fn into_raw(self) -> *mut c_void {
        let pa = self.pa;
        core::mem::forget(self);
        pa
    }

    /// Create another owned handle to the same page by incrementing the
    /// page refcount (COW-style shared ownership).
    #[inline]
    pub unsafe fn clone_shared(&self) -> Self {
        kref_inc(self.pa);
        Self { pa: self.pa }
    }
}

impl Drop for KallocPage {
    fn drop(&mut self) {
        if !self.pa.is_null() {
            unsafe { kfree(self.pa) };
        }
    }
}
