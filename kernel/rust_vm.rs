use core::ffi::{c_char, c_int, c_uint, c_void};

use crate::rust_kalloc::{kalloc, kfree, kref_inc, KallocPage};
use crate::rust_printf::panic;
use crate::rust_proc::{myproc_pagetable, myproc_sz, proc_mapstacks};
use crate::rust_string::{memset, memmove};

type Uint64 = u64;
type Pte = u64;
type Pagetable = *mut Pte;

const PGSIZE: Uint64 = 4096;
const MAXVA: Uint64 = 1u64 << (9 + 9 + 9 + 12 - 1);
const SATP_SV39: Uint64 = 8u64 << 60;
const UART0: Uint64 = 0x1000_0000;
const VIRTIO0: Uint64 = 0x1000_1000;
const PLIC: Uint64 = 0x0c00_0000;
const KERNBASE: Uint64 = 0x8000_0000;
const PHYSTOP: Uint64 = KERNBASE + 128 * 1024 * 1024;
const TRAMPOLINE: Uint64 = MAXVA - PGSIZE;
const PTE_V: Uint64 = 1 << 0;
const PTE_W: Uint64 = 1 << 2;
const PTE_X: Uint64 = 1 << 3;
const PTE_U: Uint64 = 1 << 4;
/// Reserved-for-supervisor bit 8 (RSW0) — set on a copy-on-write page.
/// Pages marked PTE_COW have PTE_W cleared and a write fault triggers
/// `vmfault` to copy the page and re-enable writes.
const PTE_COW: Uint64 = 1 << 8;
const PTE_R: c_int = 1 << 1;
const PTE_U_INT: c_int = 1 << 4;
const PTE_FLAGS_MASK: Uint64 = 0x3FF;
const PTES_PER_PT: usize = 512;

#[inline(always)]
fn pgrounddown(a: Uint64) -> Uint64 {
    a & !(PGSIZE - 1)
}

#[inline(always)]
fn pgroundup(a: Uint64) -> Uint64 {
    (a + PGSIZE - 1) & !(PGSIZE - 1)
}

#[inline(always)]
fn pte2pa(pte: Uint64) -> Uint64 {
    (pte >> 10) << 12
}

#[inline(always)]
fn pa2pte(pa: Uint64) -> Uint64 {
    (pa >> 12) << 10
}

#[inline(always)]
fn pte_flags(pte: Uint64) -> Uint64 {
    pte & PTE_FLAGS_MASK
}

#[inline(always)]
fn px(level: usize, va: Uint64) -> usize {
    ((va >> (12 + 9 * level)) & 0x1ff) as usize
}

extern "C" {
    static etext: c_char;
    static trampoline: c_char;
}

#[inline(always)]
unsafe fn panic_with(msg: &'static [u8]) -> ! {
    panic(msg.as_ptr().cast_mut().cast())
}

#[inline(always)]
unsafe fn sfence_vma() {
    core::arch::asm!("sfence.vma zero, zero");
}

#[inline(always)]
unsafe fn w_satp(x: Uint64) {
    core::arch::asm!("csrw satp, {}", in(reg) x);
}

#[inline(always)]
fn make_satp(pagetable: Pagetable) -> Uint64 {
    SATP_SV39 | ((pagetable as Uint64) >> 12)
}

#[no_mangle]
pub static mut kernel_pagetable: Pagetable = core::ptr::null_mut();

unsafe fn kvmmake() -> Pagetable {
    let kpgtbl = kalloc() as Pagetable;
    memset(kpgtbl.cast::<c_void>(), 0, PGSIZE as c_uint);

    kvmmap(kpgtbl, UART0, UART0, PGSIZE, (PTE_R as c_int) | (PTE_W as c_int));
    kvmmap(
        kpgtbl,
        VIRTIO0,
        VIRTIO0,
        PGSIZE,
        (PTE_R as c_int) | (PTE_W as c_int),
    );
    kvmmap(
        kpgtbl,
        PLIC,
        PLIC,
        0x4000000,
        (PTE_R as c_int) | (PTE_W as c_int),
    );

    let etext_addr = core::ptr::addr_of!(etext) as Uint64;
    kvmmap(
        kpgtbl,
        KERNBASE,
        KERNBASE,
        etext_addr.wrapping_sub(KERNBASE),
        (PTE_R as c_int) | (PTE_X as c_int),
    );
    kvmmap(
        kpgtbl,
        etext_addr,
        etext_addr,
        PHYSTOP.wrapping_sub(etext_addr),
        (PTE_R as c_int) | (PTE_W as c_int),
    );
    kvmmap(
        kpgtbl,
        TRAMPOLINE,
        core::ptr::addr_of!(trampoline) as Uint64,
        PGSIZE,
        (PTE_R as c_int) | (PTE_X as c_int),
    );

    proc_mapstacks(kpgtbl);
    kpgtbl
}

#[no_mangle]
pub unsafe extern "C" fn kvmmap(
    kpgtbl: Pagetable,
    va: Uint64,
    pa: Uint64,
    sz: Uint64,
    perm: c_int,
) {
    if mappages(kpgtbl, va, sz, pa, perm) != 0 {
        panic_with(b"kvmmap\0");
    }
}

#[no_mangle]
pub unsafe extern "C" fn kvminit() {
    kernel_pagetable = kvmmake();
}

#[no_mangle]
pub unsafe extern "C" fn kvminithart() {
    sfence_vma();
    w_satp(make_satp(kernel_pagetable));
    sfence_vma();
}

#[no_mangle]
pub unsafe extern "C" fn walk(mut pagetable: Pagetable, va: Uint64, alloc: c_int) -> *mut Pte {
    if va >= MAXVA {
        panic_with(b"walk\0");
    }

    for level in (1..=2).rev() {
        let pte = pagetable.add(px(level, va));
        if (*pte & PTE_V) != 0 {
            pagetable = pte2pa(*pte) as Pagetable;
        } else {
            if alloc == 0 {
                return core::ptr::null_mut();
            }
            let new_pt = kalloc();
            if new_pt.is_null() {
                return core::ptr::null_mut();
            }
            memset(new_pt, 0, PGSIZE as c_uint);
            *pte = pa2pte(new_pt as Uint64) | PTE_V;
            pagetable = new_pt as Pagetable;
        }
    }

    pagetable.add(px(0, va))
}

#[no_mangle]
pub unsafe extern "C" fn uvmcreate() -> Pagetable {
    let pagetable = kalloc() as Pagetable;
    if pagetable.is_null() {
        return core::ptr::null_mut();
    }
    memset(pagetable as *mut c_void, 0, PGSIZE as c_uint);
    pagetable
}

#[no_mangle]
pub unsafe extern "C" fn mappages(
    pagetable: Pagetable,
    va: Uint64,
    size: Uint64,
    mut pa: Uint64,
    perm: c_int,
) -> c_int {
    if (va % PGSIZE) != 0 {
        panic_with(b"mappages: va not aligned\0");
    }
    if (size % PGSIZE) != 0 {
        panic_with(b"mappages: size not aligned\0");
    }
    if size == 0 {
        panic_with(b"mappages: size\0");
    }

    let mut a = va;
    let last = va + size - PGSIZE;
    loop {
        let pte = walk(pagetable, a, 1);
        if pte.is_null() {
            return -1;
        }
        if (*pte & PTE_V) != 0 {
            panic_with(b"mappages: remap\0");
        }
        *pte = pa2pte(pa) | (perm as Uint64) | PTE_V;
        if a == last {
            break;
        }
        a += PGSIZE;
        pa += PGSIZE;
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn uvmalloc(
    pagetable: Pagetable,
    mut oldsz: Uint64,
    newsz: Uint64,
    xperm: c_int,
) -> Uint64 {
    if newsz < oldsz {
        return oldsz;
    }

    oldsz = pgroundup(oldsz);
    let mut a = oldsz;
    while a < newsz {
        let mem = kalloc();
        if mem.is_null() {
            uvmdealloc(pagetable, a, oldsz);
            return 0;
        }

        memset(mem, 0, PGSIZE as c_uint);
        if mappages(
            pagetable,
            a,
            PGSIZE,
            mem as Uint64,
            PTE_R | PTE_U_INT | xperm,
        ) != 0
        {
            kfree(mem);
            uvmdealloc(pagetable, a, oldsz);
            return 0;
        }
        a += PGSIZE;
    }

    newsz
}

#[no_mangle]
pub unsafe extern "C" fn uvmdealloc(pagetable: Pagetable, oldsz: Uint64, newsz: Uint64) -> Uint64 {
    if newsz >= oldsz {
        return oldsz;
    }

    let up_new = pgroundup(newsz);
    let up_old = pgroundup(oldsz);
    if up_new < up_old {
        let npages = (up_old - up_new) / PGSIZE;
        uvmunmap(pagetable, up_new, npages, 1);
    }

    newsz
}

/// Copy-on-write fork: instead of allocating + copying every user page
/// from `old` into `new`, share the physical pages and arrange for the
/// first write from either side to trigger a real copy. This is the
/// classic xv6 "lab COW" implementation.
///
/// For each valid leaf PTE in `old`:
///   * If the page is writable, clear PTE_W in *both* parent and child
///     and set PTE_COW so we can recognize the page on a fault.
///   * Bump the physical page's refcount; the child's PTE points at the
///     same physical page.
///
/// On error we leave the parent pagetable consistent (it may have some
/// pages newly marked COW — that's fine, the writes will still work,
/// they'll just go through `vmfault`) and unmap whatever we already
/// installed in the child.
#[no_mangle]
pub unsafe extern "C" fn uvmcopy(old: Pagetable, new: Pagetable, sz: Uint64) -> c_int {
    let mut i = 0u64;
    while i < sz {
        let pte = walk(old, i, 0);
        if pte.is_null() {
            i += PGSIZE;
            continue;
        }
        if (*pte & PTE_V) == 0 {
            i += PGSIZE;
            continue;
        }

        let pa = pte2pa(*pte);

        // Demote a writable page to COW (read-only + COW marker). If
        // the page is already read-only, leave it alone — it can be
        // shared as-is.
        if (*pte & PTE_W) != 0 {
            *pte = (*pte & !PTE_W) | PTE_COW;
        }

        let flags = pte_flags(*pte);

        if mappages(new, i, PGSIZE, pa, flags as c_int) != 0 {
            uvmunmap(new, 0, i / PGSIZE, 1);
            return -1;
        }
        // Both parent and child now reference this physical page.
        kref_inc(pa as *mut c_void);

        i += PGSIZE;
    }

    0
}

/// If `va` lives on a COW page in `pagetable`, allocate a fresh page,
/// copy the contents over, and remap with PTE_W set / PTE_COW cleared.
/// Returns the resolved physical address on success, or 0 if `va` is
/// not actually a COW page (caller should treat as a normal protection
/// fault) or if allocation fails.
unsafe fn cow_resolve(pagetable: Pagetable, va: Uint64) -> Uint64 {
    let pte = walk(pagetable, va, 0);
    if pte.is_null() {
        return 0;
    }
    let entry = *pte;
    if (entry & PTE_V) == 0 || (entry & PTE_COW) == 0 {
        return 0;
    }

    let old_pa = pte2pa(entry);
    let mem = match KallocPage::alloc() {
        Some(p) => p,
        None => return 0,
    };
    let new_pa = mem.as_ptr() as Uint64;
    memmove(mem.as_ptr(), old_pa as *const c_void, PGSIZE as c_uint);

    // Build the new PTE: drop PTE_COW, set PTE_W, retain U/R/X.
    let new_flags = (entry & PTE_FLAGS_MASK & !PTE_COW) | PTE_W;
    *pte = pa2pte(new_pa) | new_flags;

    // Drop our reference to the old page; if no one else has it, it's
    // returned to the freelist.
    kfree(old_pa as *mut c_void);

    mem.into_raw() as Uint64
}

#[no_mangle]
pub unsafe extern "C" fn freewalk(pagetable: Pagetable) {
    for i in 0..PTES_PER_PT {
        let pte_ptr = pagetable.add(i);
        let pte = *pte_ptr;
        if (pte & PTE_V) != 0 && (pte & ((PTE_R as Uint64) | PTE_W | PTE_X)) == 0 {
            let child = pte2pa(pte) as Pagetable;
            freewalk(child);
            *pte_ptr = 0;
        } else if (pte & PTE_V) != 0 {
            panic_with(b"freewalk: leaf\0");
        }
    }
    kfree(pagetable as *mut c_void);
}

#[no_mangle]
pub unsafe extern "C" fn uvmfree(pagetable: Pagetable, sz: Uint64) {
    if sz > 0 {
        uvmunmap(pagetable, 0, pgroundup(sz) / PGSIZE, 1);
    }
    freewalk(pagetable);
}

#[no_mangle]
pub unsafe extern "C" fn uvmunmap(
    pagetable: Pagetable,
    va: Uint64,
    npages: Uint64,
    do_free: c_int,
) {
    if (va % PGSIZE) != 0 {
        panic_with(b"uvmunmap: not aligned\0");
    }

    let mut a = va;
    let end = va.wrapping_add(npages.wrapping_mul(PGSIZE));
    while a < end {
        let pte = walk(pagetable, a, 0);
        if pte.is_null() {
            a += PGSIZE;
            continue;
        }
        if (*pte & PTE_V) == 0 {
            a += PGSIZE;
            continue;
        }
        if do_free != 0 {
            let pa = pte2pa(*pte);
            kfree(pa as *mut c_void);
        }
        *pte = 0;
        a += PGSIZE;
    }
}

// Look up a user virtual address and return the mapped physical page base.
#[no_mangle]
pub unsafe extern "C" fn walkaddr(pagetable: Pagetable, va: Uint64) -> Uint64 {
    if va >= MAXVA {
        return 0;
    }

    let pte = walk(pagetable, va, 0);
    if pte.is_null() {
        return 0;
    }
    let entry = *pte;
    if (entry & PTE_V) == 0 {
        return 0;
    }
    if (entry & PTE_U) == 0 {
        return 0;
    }
    pte2pa(entry)
}

// Copy from kernel to user. Kernel writes happen via the physical
// alias and bypass the W bit in the user pagetable, so we must
// explicitly resolve a COW page before writing it — otherwise the
// kernel would silently mutate a page still shared with another
// process.
#[no_mangle]
pub unsafe extern "C" fn copyout(
    pagetable: Pagetable,
    mut dstva: Uint64,
    mut src: *mut c_char,
    mut len: Uint64,
) -> c_int {
    while len > 0 {
        let va0 = pgrounddown(dstva);
        if va0 >= MAXVA {
            return -1;
        }

        let mut pa0 = walkaddr(pagetable, va0);
        if pa0 == 0 {
            pa0 = vmfault(pagetable, va0, 1);
            if pa0 == 0 {
                return -1;
            }
        }

        let pte = walk(pagetable, va0, 0);
        if pte.is_null() {
            return -1;
        }
        // If the destination is COW, take the copy now (and update pa0
        // to point at the new private page). After this, PTE_W is set.
        if (*pte & PTE_COW) != 0 {
            let new_pa = cow_resolve(pagetable, va0);
            if new_pa == 0 {
                return -1;
            }
            pa0 = new_pa;
        } else if (*pte & PTE_W) == 0 {
            // Genuinely read-only (e.g. text segment): refuse.
            return -1;
        }

        let mut n = PGSIZE - (dstva - va0);
        if n > len {
            n = len;
        }

        memmove(
            (pa0 + (dstva - va0)) as *mut c_void,
            src.cast::<c_void>(),
            n as c_uint,
        );

        len -= n;
        src = src.add(n as usize);
        dstva = va0 + PGSIZE;
    }

    0
}

// Copy from user to kernel.
#[no_mangle]
pub unsafe extern "C" fn copyin(
    pagetable: Pagetable,
    mut dst: *mut c_char,
    mut srcva: Uint64,
    mut len: Uint64,
) -> c_int {
    while len > 0 {
        let va0 = pgrounddown(srcva);
        let mut pa0 = walkaddr(pagetable, va0);
        if pa0 == 0 {
            pa0 = vmfault(pagetable, va0, 0);
            if pa0 == 0 {
                return -1;
            }
        }

        let mut n = PGSIZE - (srcva - va0);
        if n > len {
            n = len;
        }

        memmove(
            dst.cast::<c_void>(),
            (pa0 + (srcva - va0)) as *const c_void,
            n as c_uint,
        );

        len -= n;
        dst = dst.add(n as usize);
        srcva = va0 + PGSIZE;
    }

    0
}

// Copy a null-terminated string from user to kernel.
#[no_mangle]
pub unsafe extern "C" fn copyinstr(
    pagetable: Pagetable,
    mut dst: *mut c_char,
    mut srcva: Uint64,
    mut max: Uint64,
) -> c_int {
    let mut got_null = 0;

    while got_null == 0 && max > 0 {
        let va0 = pgrounddown(srcva);
        let pa0 = walkaddr(pagetable, va0);
        if pa0 == 0 {
            return -1;
        }

        let mut n = PGSIZE - (srcva - va0);
        if n > max {
            n = max;
        }

        let mut p = (pa0 + (srcva - va0)) as *const c_char;
        while n > 0 {
            if *p == 0 {
                *dst = 0;
                got_null = 1;
                break;
            } else {
                *dst = *p;
            }

            n -= 1;
            max -= 1;
            p = p.add(1);
            dst = dst.add(1);
        }

        srcva = va0 + PGSIZE;
    }

    if got_null != 0 { 0 } else { -1 }
}

#[no_mangle]
pub unsafe extern "C" fn ismapped(pagetable: Pagetable, va: Uint64) -> c_int {
    let pte = walk(pagetable, va, 0);
    if pte.is_null() {
        return 0;
    }
    if (*pte & PTE_V) != 0 { 1 } else { 0 }
}

#[no_mangle]
pub unsafe extern "C" fn uvmclear(pagetable: Pagetable, va: Uint64) {
    let pte = walk(pagetable, va, 0);
    if pte.is_null() {
        panic_with(b"uvmclear\0");
    }
    *pte &= !PTE_U;
}

/// Handle a user-mode page fault. The `is_write` flag is 1 for store
/// faults (scause == 15) and 0 for load faults (scause == 13).
///
/// Two cases handled:
///   1. Store fault on a COW page → copy the page, install a writable
///      mapping, return the new physical address.
///   2. Lazy allocation (page not yet mapped, but `va < proc.sz`) →
///      allocate a zero page and map it writable.
#[no_mangle]
pub unsafe extern "C" fn vmfault(pagetable: Pagetable, mut va: Uint64, is_write: c_int) -> Uint64 {
    let psz = myproc_sz();
    let proc_pagetable = myproc_pagetable();
    if proc_pagetable.is_null() {
        return 0;
    }
    if va >= psz {
        return 0;
    }
    va = pgrounddown(va);

    if ismapped(pagetable, va) != 0 {
        // Mapped but the access still faulted — only legitimate cause
        // is a COW page being written to. Resolve it.
        if is_write != 0 {
            return cow_resolve(pagetable, va);
        }
        return 0;
    }

    // Lazy-allocation path: produce a fresh, zeroed, writable page.
    let mem = match KallocPage::alloc() {
        Some(p) => p,
        None => return 0,
    };
    let pa = mem.as_ptr() as Uint64;
    memset(mem.as_ptr(), 0, PGSIZE as c_uint);

    if mappages(
        proc_pagetable,
        va,
        PGSIZE,
        pa,
        PTE_W as c_int | PTE_U_INT | PTE_R,
    ) != 0
    {
        return 0;
    }

    mem.into_raw() as Uint64
}
