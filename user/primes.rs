// Concurrent prime sieve, after Doug McIlroy.
//
// usage:  primes [N]      (default 280)
//
// One process per prime: each child reads ints from a pipe coming from its
// parent, prints the first one (which is prime), and forwards the rest with
// multiples of that prime filtered out, to a freshly forked grandchild.
// Demonstrates fork(), pipe(), and inter-process I/O — the canonical
// "structured concurrency in xv6" exercise.
//
// Default upper bound is small enough to stay below NPROC (=64).

#![no_std]
#![allow(dead_code)]

use core::ffi::{c_char, c_int, c_void};
use core::mem::size_of;

mod rust_user;
use rust_user::*;

const INT_SZ: c_int = size_of::<c_int>() as c_int;

/// Read one int from `fd`. Returns `Some(n)` or `None` at EOF.
unsafe fn read_int(fd: c_int) -> Option<c_int> {
    let mut n: c_int = 0;
    let r = read(fd, (&mut n as *mut c_int).cast::<c_void>(), INT_SZ);
    if r == INT_SZ {
        Some(n)
    } else {
        None
    }
}

unsafe fn write_int(fd: c_int, n: c_int) {
    write(fd, (&n as *const c_int).cast::<c_void>(), INT_SZ);
}

/// `n % p` for positive `n`, `p`. Avoids `core::panicking::*` symbols that
/// the compiler would otherwise emit for the generic `i32::%` operator.
#[inline]
fn rem_pos(n: c_int, p: c_int) -> c_int {
    let mut x = n;
    while x >= p {
        x -= p;
    }
    x
}

/// One stage in the sieve. Reads ints from `left`, prints the first (which
/// is prime), forks a child for the next stage, forwards the rest minus
/// multiples of `p`, then waits and exits.
unsafe fn sieve(left: c_int) -> ! {
    let p = match read_int(left) {
        Some(v) => v,
        None => exit(0),
    };
    printf(b"\x1b[36mprime\x1b[0m %d\n\0".as_ptr().cast(), p as u64);

    // No more numbers? Then nothing to sieve further.
    let first = match read_int(left) {
        Some(v) => v,
        None => {
            close(left);
            exit(0);
        }
    };

    // Spawn the next stage.
    let mut fds = [0 as c_int; 2];
    if pipe(fds.as_mut_ptr()) < 0 {
        fprintf(2, b"primes: pipe failed\n\0".as_ptr().cast());
        exit(1);
    }
    let pid = fork();
    if pid < 0 {
        fprintf(2, b"primes: fork failed (NPROC reached?)\n\0".as_ptr().cast());
        exit(1);
    }
    if pid == 0 {
        close(left);
        close(fds[1]);
        sieve(fds[0]);
    }

    // Parent: filter and forward.
    close(fds[0]);
    if rem_pos(first, p) != 0 {
        write_int(fds[1], first);
    }
    while let Some(n) = read_int(left) {
        if rem_pos(n, p) != 0 {
            write_int(fds[1], n);
        }
    }
    close(left);
    close(fds[1]);
    let mut s: c_int = 0;
    wait(&mut s);
    exit(0);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn main(argc: c_int, argv: *mut *mut c_char) -> c_int {
    let limit: c_int = if argc > 1 { atoi(*argv.add(1)) } else { 280 };
    if limit < 2 {
        printf(b"primes: nothing to do\n\0".as_ptr().cast());
        exit(0);
    }

    let mut fds = [0 as c_int; 2];
    if pipe(fds.as_mut_ptr()) < 0 {
        fprintf(2, b"primes: pipe failed\n\0".as_ptr().cast());
        exit(1);
    }

    let pid = fork();
    if pid < 0 {
        fprintf(2, b"primes: fork failed\n\0".as_ptr().cast());
        exit(1);
    }
    if pid == 0 {
        close(fds[1]);
        sieve(fds[0]);
    }

    // Producer: feed 2..=limit into the pipeline.
    close(fds[0]);
    let mut i: c_int = 2;
    while i <= limit {
        write_int(fds[1], i);
        i += 1;
    }
    close(fds[1]);

    let mut s: c_int = 0;
    wait(&mut s);
    printf(b"\nprimes up to %d done.\n\0".as_ptr().cast(), limit as u64);
    0
}
