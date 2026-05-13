#![no_std]

use core::ffi::{c_char, c_uint, c_void};
use core::mem::size_of;

const PAGE_UNITS: c_uint = 4096;
const SBRK_ERROR: *mut c_char = (-1isize) as *mut c_char;

#[repr(C)]
struct Header {
    ptr: *mut Header,
    size: c_uint,
}

static mut BASE: Header = Header {
    ptr: core::ptr::null_mut(),
    size: 0,
};
static mut FREEP: *mut Header = core::ptr::null_mut();

unsafe extern "C" {
    fn sbrk(n: i32) -> *mut c_char;
}

#[inline(always)]
unsafe fn header_add(p: *mut Header, n: c_uint) -> *mut Header {
    p.add(n as usize)
}

unsafe fn morecore(mut nu: c_uint) -> *mut Header {
    if nu < PAGE_UNITS {
        nu = PAGE_UNITS;
    }

    let nbytes = nu.saturating_mul(size_of::<Header>() as c_uint);
    let p = sbrk(nbytes as i32);
    if p == SBRK_ERROR {
        return core::ptr::null_mut();
    }

    let hp = p.cast::<Header>();
    (*hp).size = nu;
    free(hp.add(1).cast::<c_void>());
    FREEP
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn free(ap: *mut c_void) {
    if ap.is_null() {
        return;
    }

    let bp = (ap as *mut Header).sub(1);
    let mut p = FREEP;

    while !((bp as usize > p as usize) && ((bp as usize) < ((*p).ptr as usize))) {
        if (p as usize >= (*p).ptr as usize)
            && ((bp as usize > p as usize) || ((bp as usize) < ((*p).ptr as usize)))
        {
            break;
        }
        p = (*p).ptr;
    }

    if header_add(bp, (*bp).size) == (*p).ptr {
        (*bp).size = (*bp).size.wrapping_add((*(*p).ptr).size);
        (*bp).ptr = (*(*p).ptr).ptr;
    } else {
        (*bp).ptr = (*p).ptr;
    }

    if header_add(p, (*p).size) == bp {
        (*p).size = (*p).size.wrapping_add((*bp).size);
        (*p).ptr = (*bp).ptr;
    } else {
        (*p).ptr = bp;
    }

    FREEP = p;
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn malloc(nbytes: c_uint) -> *mut c_void {
    let nunits = (nbytes + size_of::<Header>() as c_uint - 1) / size_of::<Header>() as c_uint + 1;

    if FREEP.is_null() {
        BASE.ptr = core::ptr::addr_of_mut!(BASE);
        BASE.size = 0;
        FREEP = core::ptr::addr_of_mut!(BASE);
    }

    let mut prevp = FREEP;
    let mut p = (*prevp).ptr;

    loop {
        if (*p).size >= nunits {
            if (*p).size == nunits {
                (*prevp).ptr = (*p).ptr;
            } else {
                (*p).size -= nunits;
                p = header_add(p, (*p).size);
                (*p).size = nunits;
            }
            FREEP = prevp;
            return p.add(1).cast::<c_void>();
        }

        if p == FREEP {
            p = morecore(nunits);
            if p.is_null() {
                return core::ptr::null_mut();
            }
        }

        prevp = p;
        p = (*p).ptr;
    }
}
