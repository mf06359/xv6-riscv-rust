//! Kernel error type and functional helpers.
//!
//! xv6 historically uses `int` return values where `-1` means "something
//! went wrong" and the caller has no way to ask *what* went wrong. We
//! introduce a small algebraic data type `KError` for the kernel's
//! internal use, plus a `KResult<T>` alias so functions can compose with
//! `?`, `map`, `and_then`, `or_else`, etc.
//!
//! `KError` is converted to `c_int` (-1) at the FFI boundary so the
//! existing `extern "C"` ABI is preserved.
#![allow(dead_code)]

use core::ffi::c_int;

/// All distinguishable kernel errors that can flow back to user space
/// as `-1`. The variant carries no payload so the type is `Copy`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum KError {
    /// Invalid file descriptor, path, or argument.
    BadArg,
    /// Resource (proc table / file table / inode / disk block / page) full.
    NoResource,
    /// I/O error from the disk or another device.
    Io,
    /// User pointer points outside the process's address space.
    BadAddress,
    /// Operation interrupted by a signal / kill.
    Killed,
    /// File or directory missing.
    NotFound,
    /// Path component is not a directory where one was expected.
    NotDir,
    /// Path component is a directory where one was not expected.
    IsDir,
    /// Operation would overflow a fixed-size buffer.
    TooBig,
    /// Operation not supported (yet).
    Unsupported,
}

/// Standard alias.
pub type KResult<T> = Result<T, KError>;

// ============================================================
// FFI boundary helpers
// ============================================================

/// Convert a `KResult<c_int>` into the legacy `c_int` (-1 on Err) ABI
/// used by syscalls. Use as `to_cint(do_something())`.
#[inline]
pub fn to_cint(r: KResult<c_int>) -> c_int {
    r.unwrap_or(-1)
}

/// Convert a `KResult<u64>` into the syscall return-value convention:
/// `Ok(v)` is `v`, `Err(_)` is `(-1) as u64` (i.e. `0xffff_ffff_ffff_ffff`).
#[inline]
pub fn to_u64(r: KResult<u64>) -> u64 {
    r.unwrap_or(u64::MAX)
}

/// Lift a legacy `c_int` return where `< 0` means "error" into a
/// `KResult<c_int>` (mapping the error to a default kind).
#[inline]
pub fn from_cint(v: c_int, on_err: KError) -> KResult<c_int> {
    if v < 0 {
        Err(on_err)
    } else {
        Ok(v)
    }
}

/// Like `from_cint` but for raw pointer returns where null = error.
#[inline]
pub fn from_ptr<T>(p: *mut T, on_err: KError) -> KResult<*mut T> {
    if p.is_null() {
        Err(on_err)
    } else {
        Ok(p)
    }
}

// ============================================================
// Combinators specific to OS patterns
// ============================================================

/// Run `f` on each item until one returns `Ok(_)`; otherwise return the
/// last error (or `default_err` if the iterator was empty). Useful for
/// "try each free slot in the table" patterns.
pub fn try_first<I, T, F>(iter: I, default_err: KError, mut f: F) -> KResult<T>
where
    I: IntoIterator,
    F: FnMut(I::Item) -> KResult<T>,
{
    let mut last = Err(default_err);
    for item in iter {
        match f(item) {
            Ok(v) => return Ok(v),
            e @ Err(_) => last = e,
        }
    }
    last
}
