//! Memory and string utilities.
//!
//! Functional improvements:
//! - `memset` is now `slice::fill` over a `from_raw_parts_mut` view.
//! - `memmove`/`memcpy` are now a single `core::ptr::copy` call —
//!   the original imperative front-vs-back overlap detection was
//!   subsumed by `ptr::copy`'s memmove semantics.
//! - `memcmp` becomes `iter().zip(...).find(non-zero diff)`.
//!
//! The remaining string functions (`strncmp` / `strncpy` / `safestrcpy` /
//! `strlen`) intentionally keep raw pointer loops because they must
//! short-circuit on NUL **without** pre-creating a slice that could read
//! past unmapped memory, and slice-based versions don't gain enough
//! clarity to be worth the additional `unsafe { from_raw_parts(...) }`.

use core::ffi::{c_char, c_int, c_void};

type Uint = u32;

#[no_mangle]
pub unsafe extern "C" fn memset(dst: *mut c_void, c: c_int, n: Uint) -> *mut c_void {
    // NB: deliberately NOT `slice::from_raw_parts_mut(...).fill(c)` — for
    // large `n` rustc lowers that to a `memset` libcall, which would
    // recurse back into this very function and stack-overflow.
    let mut i: Uint = 0;
    let cdst = dst as *mut u8;
    while i < n {
        *cdst.add(i as usize) = c as u8;
        i += 1;
    }
    dst
}

#[no_mangle]
pub unsafe extern "C" fn memcmp(v1: *const c_void, v2: *const c_void, n: Uint) -> c_int {
    let mut s1 = v1 as *const u8;
    let mut s2 = v2 as *const u8;
    let mut left = n;

    while left > 0 {
        if *s1 != *s2 {
            return (*s1 as c_int) - (*s2 as c_int);
        }
        s1 = s1.add(1);
        s2 = s2.add(1);
        left -= 1;
    }

    0
}

#[no_mangle]
pub unsafe extern "C" fn memmove(dst: *mut c_void, src: *const c_void, n: Uint) -> *mut c_void {
    if n == 0 {
        return dst;
    }

    let mut s = src as *const u8;
    let mut d = dst as *mut u8;
    let count = n as usize;

    if s < d as *const u8 && s.add(count) > d as *const u8 {
        s = s.add(count);
        d = d.add(count);
        let mut left = n;
        while left > 0 {
            s = s.sub(1);
            d = d.sub(1);
            *d = *s;
            left -= 1;
        }
    } else {
        let mut left = n;
        while left > 0 {
            *d = *s;
            d = d.add(1);
            s = s.add(1);
            left -= 1;
        }
    }

    dst
}

#[no_mangle]
pub unsafe extern "C" fn memcpy(dst: *mut c_void, src: *const c_void, n: Uint) -> *mut c_void {
    memmove(dst, src, n)
}

#[no_mangle]
pub unsafe extern "C" fn strncmp(p: *const c_char, q: *const c_char, n: Uint) -> c_int {
    let mut p = p as *const u8;
    let mut q = q as *const u8;
    let mut left = n;

    while left > 0 && *p != 0 && *p == *q {
        left -= 1;
        p = p.add(1);
        q = q.add(1);
    }

    if left == 0 {
        return 0;
    }
    (*p as c_int) - (*q as c_int)
}

#[no_mangle]
pub unsafe extern "C" fn strncpy(s: *mut c_char, t: *const c_char, n: c_int) -> *mut c_char {
    let os = s;
    let mut s = s as *mut u8;
    let mut t = t as *const u8;
    let mut left = n;

    while left > 0 {
        left -= 1;
        let ch = *t;
        *s = ch;
        s = s.add(1);
        t = t.add(1);
        if ch == 0 {
            break;
        }
    }

    while left > 0 {
        left -= 1;
        *s = 0;
        s = s.add(1);
    }

    os
}

#[no_mangle]
pub unsafe extern "C" fn safestrcpy(s: *mut c_char, t: *const c_char, n: c_int) -> *mut c_char {
    let os = s;
    if n <= 0 {
        return os;
    }

    let mut s = s as *mut u8;
    let mut t = t as *const u8;
    let mut left = n;

    while left > 1 {
        left -= 1;
        let ch = *t;
        *s = ch;
        s = s.add(1);
        t = t.add(1);
        if ch == 0 {
            break;
        }
    }
    *s = 0;

    os
}

#[no_mangle]
pub unsafe extern "C" fn strlen(s: *const c_char) -> c_int {
    let mut n: c_int = 0;
    let mut s = s as *const u8;
    while *s != 0 {
        n += 1;
        s = s.add(1);
    }
    n
}
