#![no_std]
#![allow(dead_code, non_snake_case, non_upper_case_globals, unused_assignments)]

use core::ffi::{c_char, c_int, c_void};
use core::ptr::{addr_of, addr_of_mut, null_mut};

mod rust_user;
use rust_user::*;

// ============== macros ==============

macro_rules! c {
    ($s:expr) => { concat!($s, "\0").as_ptr() as *const c_char };
}

macro_rules! pf {
    ($fmt:expr $(, $args:expr)* $(,)?) => {
        printf(c!($fmt) $(, $args)*)
    };
}

macro_rules! pft {
    ($s:expr, $fmt:expr $(, $args:expr)* $(,)?) => {
        printf(concat!("%s: ", $fmt, "\0").as_ptr() as *const c_char,
               $s as u64 $(, $args)*)
    };
}

macro_rules! die {
    ($($t:tt)*) => {{ pf!($($t)*); exit(1) }};
}

macro_rules! diet {
    ($s:expr, $fmt:expr $(, $args:expr)* $(,)?) => {{
        pft!($s, $fmt $(, $args)*); exit(1)
    }};
}

macro_rules! t {
    ($name:expr, $f:ident) => { Test { f: $f, s: concat!($name, "\0").as_bytes() } };
}

// ============== globals & helpers ==============

const BUFSZ: usize = (MAXOPBLOCKS + 2) * BSIZE;
static mut BUF: [u8; BUFSZ] = [0; BUFSZ];

#[inline(always)]
unsafe fn buf_ptr() -> *mut c_void { addr_of_mut!(BUF).cast() }
#[inline(always)]
unsafe fn buf_const_ptr() -> *const c_void { addr_of!(BUF).cast() }
#[inline(always)]
unsafe fn buf_get(i: usize) -> u8 { *(addr_of!(BUF) as *const u8).add(i) }
#[inline(always)]
unsafe fn buf_set(i: usize, v: u8) { *(addr_of_mut!(BUF) as *mut u8).add(i) = v; }

#[inline(always)]
unsafe fn r_sp() -> u64 {
    let x: u64;
    core::arch::asm!("mv {x}, sp", x = out(reg) x);
    x
}

#[inline(always)]
unsafe fn wait1() -> c_int {
    let mut s: c_int = 0;
    wait(&mut s);
    s
}

#[inline(always)]
unsafe fn fork_or_die() -> c_int {
    let pid = fork();
    if pid < 0 { die!("fork failed\n"); }
    pid
}

// ============== tests ==============

unsafe fn copyin(_s: *const c_char) {
    let addrs: [u64; 5] = [0x80000000, 0x3fffffe000, 0x3ffffff000, 0x4000000000, !0];
    for &addr in addrs.iter() {
        let fd = open(c!("copyin1"), O_CREATE | O_WRONLY);
        if fd < 0 { die!("open(copyin1) failed\n"); }
        let n = write(fd, addr as *const c_void, 8192);
        if n >= 0 { die!("write(fd, %p, 8192) returned %d, not -1\n", addr, n as u64); }
        close(fd);
        unlink(c!("copyin1"));

        let n = write(1, addr as *const c_void, 8192);
        if n > 0 { die!("write(1, %p, 8192) returned %d, not -1 or 0\n", addr, n as u64); }

        let mut fds = [0 as c_int; 2];
        if pipe(fds.as_mut_ptr()) < 0 { die!("pipe() failed\n"); }
        let n = write(fds[1], addr as *const c_void, 8192);
        if n > 0 { die!("write(pipe, %p, 8192) returned %d, not -1 or 0\n", addr, n as u64); }
        close(fds[0]);
        close(fds[1]);
    }
}

unsafe fn copyout(_s: *const c_char) {
    let addrs: [u64; 6] = [0, 0x80000000, 0x3fffffe000, 0x3ffffff000, 0x4000000000, !0];
    for &addr in addrs.iter() {
        let fd = open(c!("README"), 0);
        if fd < 0 { die!("open(README) failed\n"); }
        let n = read(fd, addr as *mut c_void, 8192);
        if n > 0 { die!("read(fd, %p, 8192) returned %d, not -1 or 0\n", addr, n as u64); }
        close(fd);

        let mut fds = [0 as c_int; 2];
        if pipe(fds.as_mut_ptr()) < 0 { die!("pipe() failed\n"); }
        if write(fds[1], c!("x") as *const c_void, 1) != 1 { die!("pipe write failed\n"); }
        let n = read(fds[0], addr as *mut c_void, 8192);
        if n > 0 { die!("read(pipe, %p, 8192) returned %d, not -1 or 0\n", addr, n as u64); }
        close(fds[0]);
        close(fds[1]);
    }
}

unsafe fn copyinstr1(_s: *const c_char) {
    let addrs: [u64; 5] = [0x80000000, 0x3fffffe000, 0x3ffffff000, 0x4000000000, !0];
    for &addr in addrs.iter() {
        let fd = open(addr as *const c_char, O_CREATE | O_WRONLY);
        if fd >= 0 { die!("open(%p) returned %d, not -1\n", addr, fd as u64); }
    }
}

unsafe fn copyinstr2(_s: *const c_char) {
    let mut b = [b'x'; MAXPATH + 1];
    b[MAXPATH] = 0;
    let bp = b.as_ptr() as *const c_char;

    let r = unlink(bp);
    if r != -1 { die!("unlink(%s) returned %d, not -1\n", bp as u64, r as u64); }
    let fd = open(bp, O_CREATE | O_WRONLY);
    if fd != -1 { die!("open(%s) returned %d, not -1\n", bp as u64, fd as u64); }
    let r = link(bp, bp);
    if r != -1 { die!("link(%s, %s) returned %d, not -1\n", bp as u64, bp as u64, r as u64); }

    let mut args = [c!("xx") as *mut c_char, null_mut()];
    let r = exec(bp, args.as_mut_ptr());
    if r != -1 { die!("exec(%s) returned %d, not -1\n", bp as u64, fd as u64); }

    let pid = fork_or_die();
    if pid == 0 {
        static mut BIG: [u8; (PGSIZE as usize) + 1] = [0; (PGSIZE as usize) + 1];
        let bigp = addr_of_mut!(BIG) as *mut u8;
        for i in 0..(PGSIZE as usize) { *bigp.add(i) = b'x'; }
        *bigp.add(PGSIZE as usize) = 0;
        let mut a2 = [bigp as *mut c_char, bigp as *mut c_char, bigp as *mut c_char, null_mut()];
        let r = exec(c!("echo"), a2.as_mut_ptr());
        if r != -1 { die!("exec(echo, BIG) returned %d, not -1\n", fd as u64); }
        exit(747);
    }
    if wait1() != 747 { die!("exec(echo, BIG) succeeded, should have failed\n"); }
}

unsafe fn copyinstr3(_s: *const c_char) {
    sbrk(8192);
    let top = sbrk(0) as u64;
    if top % PGSIZE != 0 { sbrk((PGSIZE - top % PGSIZE) as c_int); }
    if (sbrk(0) as u64) % PGSIZE != 0 { die!("oops\n"); }

    let b = (sbrk(0) as u64 - 1) as *mut c_char;
    *b = b'x' as c_char;

    let r = unlink(b);
    if r != -1 { die!("unlink(%s) returned %d, not -1\n", b as u64, r as u64); }
    let fd = open(b, O_CREATE | O_WRONLY);
    if fd != -1 { die!("open(%s) returned %d, not -1\n", b as u64, fd as u64); }
    let r = link(b, b);
    if r != -1 { die!("link(%s, %s) returned %d, not -1\n", b as u64, b as u64, r as u64); }

    let mut args = [c!("xx") as *mut c_char, null_mut()];
    let r = exec(b, args.as_mut_ptr());
    if r != -1 { die!("exec(%s) returned %d, not -1\n", b as u64, fd as u64); }
}

unsafe fn rwsbrk(_s: *const c_char) {
    let a = sbrk(8192) as u64;
    if a == SBRK_ERROR as u64 { die!("sbrk(rwsbrk) failed\n"); }
    if sbrk(-8192) == SBRK_ERROR { die!("sbrk(rwsbrk) shrink failed\n"); }

    let fd = open(c!("rwsbrk"), O_CREATE | O_WRONLY);
    if fd < 0 { die!("open(rwsbrk) failed\n"); }
    let n = write(fd, (a + PGSIZE) as *const c_void, 1024);
    if n >= 0 { die!("write(fd, %p, 1024) returned %d, not -1\n", a + PGSIZE, n as u64); }
    close(fd);
    unlink(c!("rwsbrk"));

    let fd = open(c!("README"), O_RDONLY);
    if fd < 0 { die!("open(README) failed\n"); }
    let n = read(fd, (a + PGSIZE) as *mut c_void, 10);
    if n >= 0 { die!("read(fd, %p, 10) returned %d, not -1\n", a + PGSIZE, n as u64); }
    close(fd);
    exit(0);
}

unsafe fn truncate1(s: *const c_char) {
    let mut tb = [0u8; 32];
    let tbp = tb.as_mut_ptr().cast::<c_void>();
    let tblen = tb.len() as c_int;

    unlink(c!("truncfile"));
    let fd1 = open(c!("truncfile"), O_CREATE | O_WRONLY | O_TRUNC);
    write(fd1, c!("abcd") as *const c_void, 4);
    close(fd1);

    let fd2 = open(c!("truncfile"), O_RDONLY);
    let n = read(fd2, tbp, tblen);
    if n != 4 { diet!(s, "read %d bytes, wanted 4\n", n as u64); }

    let fd1 = open(c!("truncfile"), O_WRONLY | O_TRUNC);

    let fd3 = open(c!("truncfile"), O_RDONLY);
    let n = read(fd3, tbp, tblen);
    if n != 0 {
        pf!("aaa fd3=%d\n", fd3 as u64);
        diet!(s, "read %d bytes, wanted 0\n", n as u64);
    }

    let n = read(fd2, tbp, tblen);
    if n != 0 {
        pf!("bbb fd2=%d\n", fd2 as u64);
        diet!(s, "read %d bytes, wanted 0\n", n as u64);
    }

    write(fd1, c!("abcdef") as *const c_void, 6);

    let n = read(fd3, tbp, tblen);
    if n != 6 { diet!(s, "read %d bytes, wanted 6\n", n as u64); }
    let n = read(fd2, tbp, tblen);
    if n != 2 { diet!(s, "read %d bytes, wanted 2\n", n as u64); }

    unlink(c!("truncfile"));
    close(fd1); close(fd2); close(fd3);
}

unsafe fn truncate2(s: *const c_char) {
    unlink(c!("truncfile"));
    let fd1 = open(c!("truncfile"), O_CREATE | O_TRUNC | O_WRONLY);
    write(fd1, c!("abcd") as *const c_void, 4);
    let fd2 = open(c!("truncfile"), O_TRUNC | O_WRONLY);
    let n = write(fd1, c!("x") as *const c_void, 1);
    if n != -1 { diet!(s, "write returned %d, expected -1\n", n as u64); }
    unlink(c!("truncfile"));
    close(fd1); close(fd2);
}

unsafe fn truncate3(s: *const c_char) {
    close(open(c!("truncfile"), O_CREATE | O_TRUNC | O_WRONLY));

    let pid = fork_or_die();
    if pid == 0 {
        for _ in 0..100 {
            let mut tb = [0u8; 32];
            let fd = open(c!("truncfile"), O_WRONLY);
            if fd < 0 { diet!(s, "open failed\n"); }
            let n = write(fd, c!("1234567890") as *const c_void, 10);
            if n != 10 { diet!(s, "write got %d, expected 10\n", n as u64); }
            close(fd);
            let fd = open(c!("truncfile"), O_RDONLY);
            read(fd, tb.as_mut_ptr().cast(), tb.len() as c_int);
            close(fd);
        }
        exit(0);
    }

    for _ in 0..150 {
        let fd = open(c!("truncfile"), O_CREATE | O_WRONLY | O_TRUNC);
        if fd < 0 { diet!(s, "open failed\n"); }
        let n = write(fd, c!("xxx") as *const c_void, 3);
        if n != 3 { diet!(s, "write got %d, expected 3\n", n as u64); }
        close(fd);
    }

    let xs = wait1();
    unlink(c!("truncfile"));
    exit(xs);
}

unsafe fn iputtest(s: *const c_char) {
    if mkdir(c!("iputdir")) < 0 { diet!(s, "mkdir failed\n"); }
    if chdir(c!("iputdir")) < 0 { diet!(s, "chdir iputdir failed\n"); }
    if unlink(c!("../iputdir")) < 0 { diet!(s, "unlink ../iputdir failed\n"); }
    if chdir(c!("/")) < 0 { diet!(s, "chdir / failed\n"); }
}

unsafe fn exitiput(s: *const c_char) {
    let pid = fork_or_die();
    if pid == 0 {
        if mkdir(c!("iputdir")) < 0 { diet!(s, "mkdir failed\n"); }
        if chdir(c!("iputdir")) < 0 { diet!(s, "child chdir failed\n"); }
        if unlink(c!("../iputdir")) < 0 { diet!(s, "unlink ../iputdir failed\n"); }
        exit(0);
    }
    exit(wait1());
}

unsafe fn openiput(s: *const c_char) {
    if mkdir(c!("oidir")) < 0 { diet!(s, "mkdir oidir failed\n"); }
    let pid = fork_or_die();
    if pid == 0 {
        let fd = open(c!("oidir"), O_RDWR);
        if fd >= 0 { diet!(s, "open directory for write succeeded\n"); }
        exit(0);
    }
    pause(1);
    if unlink(c!("oidir")) != 0 { diet!(s, "unlink failed\n"); }
    exit(wait1());
}

unsafe fn opentest(s: *const c_char) {
    let fd = open(c!("echo"), 0);
    if fd < 0 { diet!(s, "open echo failed!\n"); }
    close(fd);
    if open(c!("doesnotexist"), 0) >= 0 { diet!(s, "open doesnotexist succeeded!\n"); }
}

unsafe fn writetest(s: *const c_char) {
    const N: c_int = 100;
    const SZ: c_int = 10;
    let fd = open(c!("small"), O_CREATE | O_RDWR);
    if fd < 0 { diet!(s, "error: creat small failed!\n"); }
    for i in 0..N {
        if write(fd, c!("aaaaaaaaaa") as *const c_void, SZ) != SZ {
            diet!(s, "error: write aa %d new file failed\n", i as u64);
        }
        if write(fd, c!("bbbbbbbbbb") as *const c_void, SZ) != SZ {
            diet!(s, "error: write bb %d new file failed\n", i as u64);
        }
    }
    close(fd);
    let fd = open(c!("small"), O_RDONLY);
    if fd < 0 { diet!(s, "error: open small failed!\n"); }
    if read(fd, buf_ptr(), N * SZ * 2) != N * SZ * 2 { diet!(s, "read failed\n"); }
    close(fd);
    if unlink(c!("small")) < 0 { diet!(s, "unlink small failed\n"); }
}

unsafe fn writebig(s: *const c_char) {
    let fd = open(c!("big"), O_CREATE | O_RDWR);
    if fd < 0 { diet!(s, "error: creat big failed!\n"); }
    for i in 0..(MAXFILE as c_int) {
        *(addr_of_mut!(BUF) as *mut c_int) = i;
        if write(fd, buf_const_ptr(), BSIZE as c_int) != BSIZE as c_int {
            diet!(s, "error: write big file failed i=%d\n", i as u64);
        }
    }
    close(fd);

    let fd = open(c!("big"), O_RDONLY);
    if fd < 0 { diet!(s, "error: open big failed!\n"); }
    let mut n: c_int = 0;
    loop {
        let i = read(fd, buf_ptr(), BSIZE as c_int);
        if i == 0 {
            if n != MAXFILE as c_int { diet!(s, "read only %d blocks from big", n as u64); }
            break;
        } else if i != BSIZE as c_int {
            diet!(s, "read failed %d\n", i as u64);
        }
        let v = *(addr_of!(BUF) as *const c_int);
        if v != n { diet!(s, "read content of block %d is %d\n", n as u64, v as u64); }
        n += 1;
    }
    close(fd);
    if unlink(c!("big")) < 0 { diet!(s, "unlink big failed\n"); }
}

unsafe fn createtest(_s: *const c_char) {
    const N: c_int = 52;
    let mut name = [b'a', 0, 0];
    for i in 0..N {
        name[1] = b'0' + i as u8;
        close(open(name.as_ptr().cast(), O_CREATE | O_RDWR));
    }
    for i in 0..N {
        name[1] = b'0' + i as u8;
        unlink(name.as_ptr().cast());
    }
}

unsafe fn dirtest(s: *const c_char) {
    if mkdir(c!("dir0")) < 0 { diet!(s, "mkdir failed\n"); }
    if chdir(c!("dir0")) < 0 { diet!(s, "chdir dir0 failed\n"); }
    if chdir(c!("..")) < 0 { diet!(s, "chdir .. failed\n"); }
    if unlink(c!("dir0")) < 0 { diet!(s, "unlink dir0 failed\n"); }
}

unsafe fn exectest(s: *const c_char) {
    let mut argv = [c!("echo") as *mut c_char, c!("OK") as *mut c_char, null_mut()];
    let mut tb = [0u8; 3];

    unlink(c!("echo-ok"));
    let pid = fork_or_die();
    if pid == 0 {
        close(1);
        let fd = open(c!("echo-ok"), O_CREATE | O_WRONLY);
        if fd < 0 { diet!(s, "create failed\n"); }
        if fd != 1 { diet!(s, "wrong fd\n"); }
        if exec(c!("echo"), argv.as_mut_ptr()) < 0 { diet!(s, "exec echo failed\n"); }
    }
    let mut xs: c_int = 0;
    if wait(&mut xs) != pid { pft!(s, "wait failed!\n"); }
    if xs != 0 { exit(xs); }

    let fd = open(c!("echo-ok"), O_RDONLY);
    if fd < 0 { diet!(s, "open failed\n"); }
    if read(fd, tb.as_mut_ptr().cast(), 2) != 2 { diet!(s, "read failed\n"); }
    unlink(c!("echo-ok"));
    if tb[0] == b'O' && tb[1] == b'K' { exit(0); }
    diet!(s, "wrong output\n");
}

unsafe fn pipe1(s: *const c_char) {
    const N: c_int = 5;
    const SZ: c_int = 1033;
    let mut fds = [0 as c_int; 2];
    if pipe(fds.as_mut_ptr()) != 0 { diet!(s, "pipe() failed\n"); }
    let pid = fork();
    let mut seq: c_int = 0;
    if pid == 0 {
        close(fds[0]);
        for _ in 0..N {
            for i in 0..SZ {
                buf_set(i as usize, seq as u8);
                seq += 1;
            }
            if write(fds[1], buf_const_ptr(), SZ) != SZ { diet!(s, "pipe1 oops 1\n"); }
        }
        exit(0);
    } else if pid > 0 {
        close(fds[1]);
        let mut total: c_int = 0;
        let mut cc: c_int = 1;
        loop {
            let n = read(fds[0], buf_ptr(), cc);
            if n <= 0 { break; }
            for i in 0..n {
                if (buf_get(i as usize) & 0xff) != ((seq as u8) & 0xff) {
                    pft!(s, "pipe1 oops 2\n");
                    return;
                }
                seq += 1;
            }
            total += n;
            cc *= 2;
            if cc as usize > BUFSZ { cc = BUFSZ as c_int; }
        }
        if total != N * SZ { diet!(s, "pipe1 oops 3 total %d\n", total as u64); }
        close(fds[0]);
        exit(wait1());
    }
    diet!(s, "fork() failed\n");
}

unsafe fn killstatus(s: *const c_char) {
    for _ in 0..100 {
        let p = fork_or_die();
        if p == 0 { loop { getpid(); } }
        pause(1);
        kill(p);
        if wait1() != -1 { diet!(s, "status should be -1\n"); }
    }
    exit(0);
}

unsafe fn preempt(s: *const c_char) {
    let p1 = fork_or_die();
    if p1 == 0 { loop {} }
    let p2 = fork_or_die();
    if p2 == 0 { loop {} }

    let mut pfds = [0 as c_int; 2];
    pipe(pfds.as_mut_ptr());
    let p3 = fork_or_die();
    if p3 == 0 {
        close(pfds[0]);
        if write(pfds[1], c!("x") as *const c_void, 1) != 1 { pft!(s, "preempt write error"); }
        close(pfds[1]);
        loop {}
    }

    close(pfds[1]);
    if read(pfds[0], buf_ptr(), BUFSZ as c_int) != 1 { pft!(s, "preempt read error"); return; }
    close(pfds[0]);
    pf!("kill... ");
    kill(p1); kill(p2); kill(p3);
    pf!("wait... ");
    wait(null_mut()); wait(null_mut()); wait(null_mut());
}

unsafe fn exitwait(s: *const c_char) {
    for i in 0..100 {
        let pid = fork_or_die();
        if pid != 0 {
            let mut xs: c_int = 0;
            if wait(&mut xs) != pid { diet!(s, "wait wrong pid\n"); }
            if i != xs { diet!(s, "wait wrong exit status\n"); }
        } else {
            exit(i);
        }
    }
}

unsafe fn reparent(s: *const c_char) {
    let master = getpid();
    for _ in 0..200 {
        let pid = fork_or_die();
        if pid != 0 {
            if wait(null_mut()) != pid { diet!(s, "wait wrong pid\n"); }
        } else {
            if fork() < 0 { kill(master); exit(1); }
            exit(0);
        }
    }
    exit(0);
}

unsafe fn twochildren(_s: *const c_char) {
    for _ in 0..1000 {
        let p1 = fork_or_die();
        if p1 == 0 { exit(0); }
        let p2 = fork_or_die();
        if p2 == 0 { exit(0); }
        wait(null_mut()); wait(null_mut());
    }
}

unsafe fn forkfork(s: *const c_char) {
    const N: c_int = 2;
    for _ in 0..N {
        let p = fork_or_die();
        if p == 0 {
            for _ in 0..200 {
                let p1 = fork();
                if p1 < 0 { exit(1); }
                if p1 == 0 { exit(0); }
                wait(null_mut());
            }
            exit(0);
        }
    }
    for _ in 0..N {
        if wait1() != 0 { diet!(s, "fork in child failed"); }
    }
}

unsafe fn forkforkfork(s: *const c_char) {
    unlink(c!("stopforking"));
    let p = fork_or_die();
    if p == 0 {
        loop {
            if open(c!("stopforking"), 0) >= 0 { exit(0); }
            if fork() < 0 { close(open(c!("stopforking"), O_CREATE | O_RDWR)); }
        }
    }
    pause(20);
    close(open(c!("stopforking"), O_CREATE | O_RDWR));
    wait(null_mut());
    pause(10);
    let _ = s;
}

unsafe fn reparent2(_s: *const c_char) {
    for _ in 0..800 {
        let p = fork();
        if p < 0 { die!("fork failed\n"); }
        if p == 0 { fork(); fork(); exit(0); }
        wait(null_mut());
    }
    exit(0);
}

unsafe fn mem(s: *const c_char) {
    let pid = fork();
    if pid == 0 {
        let mut m1: *mut c_void = null_mut();
        loop {
            let m2 = malloc(10001);
            if m2.is_null() { break; }
            *(m2 as *mut *mut c_void) = m1;
            m1 = m2;
        }
        while !m1.is_null() {
            let m2 = *(m1 as *mut *mut c_void);
            free(m1);
            m1 = m2;
        }
        let m1 = malloc(1024 * 20);
        if m1.is_null() { diet!(s, "couldn't allocate mem?!!\n"); }
        free(m1);
        exit(0);
    }
    let xs = wait1();
    if xs == -1 { exit(0); }
    exit(xs);
}

unsafe fn sharedfd(s: *const c_char) {
    const N: c_int = 1000;
    const SZ: usize = 10;
    let mut tb = [0u8; SZ];

    unlink(c!("sharedfd"));
    let fd = open(c!("sharedfd"), O_CREATE | O_RDWR);
    if fd < 0 { diet!(s, "cannot open sharedfd for writing"); }
    let pid = fork();
    let c = if pid == 0 { b'c' } else { b'p' };
    for v in tb.iter_mut() { *v = c; }
    for _ in 0..N {
        if write(fd, tb.as_ptr().cast(), SZ as c_int) != SZ as c_int {
            diet!(s, "write sharedfd failed\n");
        }
    }
    if pid == 0 { exit(0); }
    let xs = wait1();
    if xs != 0 { exit(xs); }

    close(fd);
    let fd = open(c!("sharedfd"), 0);
    if fd < 0 { diet!(s, "cannot open sharedfd for reading\n"); }
    let mut nc: c_int = 0;
    let mut np: c_int = 0;
    loop {
        let n = read(fd, tb.as_mut_ptr().cast(), SZ as c_int);
        if n <= 0 { break; }
        for i in 0..(n as usize) {
            let ch = *tb.as_ptr().add(i);
            if ch == b'c' { nc += 1; }
            if ch == b'p' { np += 1; }
        }
    }
    close(fd);
    unlink(c!("sharedfd"));
    if nc == N * SZ as c_int && np == N * SZ as c_int { exit(0); }
    diet!(s, "nc/np test fails\n");
}

unsafe fn fourfiles(s: *const c_char) {
    const N: c_int = 12;
    const NCHILD: c_int = 4;
    const SZ: c_int = 500;
    let names = [c!("f0"), c!("f1"), c!("f2"), c!("f3")];

    for pi in 0..NCHILD {
        let fname = names[pi as usize];
        unlink(fname);
        let pid = fork_or_die();
        if pid == 0 {
            let fd = open(fname, O_CREATE | O_RDWR);
            if fd < 0 { diet!(s, "create failed\n"); }
            for i in 0..(SZ as usize) { buf_set(i, b'0' + pi as u8); }
            for _ in 0..N {
                let n = write(fd, buf_const_ptr(), SZ);
                if n != SZ { die!("write failed %d\n", n as u64); }
            }
            exit(0);
        }
    }

    for _ in 0..NCHILD {
        let xs = wait1();
        if xs != 0 { exit(xs); }
    }

    for i in 0..NCHILD {
        let fname = names[i as usize];
        let fd = open(fname, 0);
        let mut total: c_int = 0;
        loop {
            let n = read(fd, buf_ptr(), BUFSZ as c_int);
            if n <= 0 { break; }
            for j in 0..(n as usize) {
                if buf_get(j) != b'0' + i as u8 { diet!(s, "wrong char\n"); }
            }
            total += n;
        }
        close(fd);
        if total != N * SZ { die!("wrong length %d\n", total as u64); }
        unlink(fname);
    }
}

unsafe fn createdelete(s: *const c_char) {
    const N: c_int = 20;
    const NCHILD: c_int = 4;
    let mut name = [0u8; 32];

    for pi in 0..NCHILD {
        let pid = fork_or_die();
        if pid == 0 {
            name[0] = b'p' + pi as u8;
            for i in 0..N {
                name[1] = b'0' + i as u8;
                let fd = open(name.as_ptr().cast(), O_CREATE | O_RDWR);
                if fd < 0 { diet!(s, "create failed\n"); }
                close(fd);
                if i > 0 && i % 2 == 0 {
                    name[1] = b'0' + (i / 2) as u8;
                    if unlink(name.as_ptr().cast()) < 0 { diet!(s, "unlink failed\n"); }
                }
            }
            exit(0);
        }
    }

    for _ in 0..NCHILD {
        if wait1() != 0 { exit(1); }
    }

    name = [0u8; 32];
    for i in 0..N {
        for pi in 0..NCHILD {
            name[0] = b'p' + pi as u8;
            name[1] = b'0' + i as u8;
            let fd = open(name.as_ptr().cast(), 0);
            if (i == 0 || i >= N / 2) && fd < 0 {
                diet!(s, "oops createdelete %s didn't exist\n", name.as_ptr() as u64);
            } else if i >= 1 && i < N / 2 && fd >= 0 {
                diet!(s, "oops createdelete %s did exist\n", name.as_ptr() as u64);
            }
            if fd >= 0 { close(fd); }
        }
    }

    for i in 0..N {
        for pi in 0..NCHILD {
            name[0] = b'p' + pi as u8;
            name[1] = b'0' + i as u8;
            unlink(name.as_ptr().cast());
        }
    }
}

unsafe fn unlinkread(s: *const c_char) {
    const SZ: c_int = 5;
    let fd = open(c!("unlinkread"), O_CREATE | O_RDWR);
    if fd < 0 { diet!(s, "create unlinkread failed\n"); }
    write(fd, c!("hello") as *const c_void, SZ);
    close(fd);

    let fd = open(c!("unlinkread"), O_RDWR);
    if fd < 0 { diet!(s, "open unlinkread failed\n"); }
    if unlink(c!("unlinkread")) != 0 { diet!(s, "unlink unlinkread failed\n"); }

    let fd1 = open(c!("unlinkread"), O_CREATE | O_RDWR);
    write(fd1, c!("yyy") as *const c_void, 3);
    close(fd1);

    if read(fd, buf_ptr(), BUFSZ as c_int) != SZ { diet!(s, "unlinkread read failed"); }
    if buf_get(0) != b'h' { diet!(s, "unlinkread wrong data\n"); }
    if write(fd, buf_const_ptr(), 10) != 10 { diet!(s, "unlinkread write failed\n"); }
    close(fd);
    unlink(c!("unlinkread"));
}

unsafe fn linktest(s: *const c_char) {
    const SZ: c_int = 5;
    unlink(c!("lf1"));
    unlink(c!("lf2"));

    let fd = open(c!("lf1"), O_CREATE | O_RDWR);
    if fd < 0 { diet!(s, "create lf1 failed\n"); }
    if write(fd, c!("hello") as *const c_void, SZ) != SZ { diet!(s, "write lf1 failed\n"); }
    close(fd);

    if link(c!("lf1"), c!("lf2")) < 0 { diet!(s, "link lf1 lf2 failed\n"); }
    unlink(c!("lf1"));

    if open(c!("lf1"), 0) >= 0 { diet!(s, "unlinked lf1 but it is still there!\n"); }

    let fd = open(c!("lf2"), 0);
    if fd < 0 { diet!(s, "open lf2 failed\n"); }
    if read(fd, buf_ptr(), BUFSZ as c_int) != SZ { diet!(s, "read lf2 failed\n"); }
    close(fd);

    if link(c!("lf2"), c!("lf2")) >= 0 { diet!(s, "link lf2 lf2 succeeded! oops\n"); }
    unlink(c!("lf2"));
    if link(c!("lf2"), c!("lf1")) >= 0 { diet!(s, "link non-existent succeeded! oops\n"); }
    if link(c!("."), c!("lf1")) >= 0 { diet!(s, "link . lf1 succeeded! oops\n"); }
}

#[repr(C)]
struct ConcDe { inum: u16, name: [c_char; DIRSIZ] }

unsafe fn concreate(s: *const c_char) {
    const N: usize = 40;
    let mut file = [b'C', 0, 0];
    let mut fa = [0u8; N];
    let mut de = ConcDe { inum: 0, name: [0; DIRSIZ] };

    for i in 0..(N as c_int) {
        file[1] = b'0' + i as u8;
        unlink(file.as_ptr().cast());
        let pid = fork();
        if pid != 0 && i % 3 == 1 {
            link(c!("C0"), file.as_ptr().cast());
        } else if pid == 0 && i % 5 == 1 {
            link(c!("C0"), file.as_ptr().cast());
        } else {
            let fd = open(file.as_ptr().cast(), O_CREATE | O_RDWR);
            if fd < 0 { die!("concreate create %s failed\n", file.as_ptr() as u64); }
            close(fd);
        }
        if pid == 0 { exit(0); }
        if wait1() != 0 { exit(1); }
    }

    let fd = open(c!("."), 0);
    let mut n: c_int = 0;
    loop {
        let r = read(fd, (&mut de as *mut ConcDe).cast(), core::mem::size_of::<ConcDe>() as c_int);
        if r <= 0 { break; }
        if de.inum == 0 { continue; }
        if de.name[0] as u8 == b'C' && de.name[2] == 0 {
            let i = (de.name[1] as u8 - b'0') as i32;
            if i < 0 || i as usize >= fa.len() {
                diet!(s, "concreate weird file %s\n", de.name.as_ptr() as u64);
            }
            if *fa.as_ptr().add(i as usize) != 0 {
                diet!(s, "concreate duplicate file %s\n", de.name.as_ptr() as u64);
            }
            *fa.as_mut_ptr().add(i as usize) = 1;
            n += 1;
        }
    }
    close(fd);
    if n != N as c_int { diet!(s, "concreate not enough files in directory listing\n"); }

    for i in 0..(N as c_int) {
        file[1] = b'0' + i as u8;
        let pid = fork_or_die();
        if (i % 3 == 0 && pid == 0) || (i % 3 == 1 && pid != 0) {
            for _ in 0..6 { close(open(file.as_ptr().cast(), 0)); }
        } else {
            for _ in 0..6 { unlink(file.as_ptr().cast()); }
        }
        if pid == 0 { exit(0); }
        wait(null_mut());
    }
}

unsafe fn linkunlink(s: *const c_char) {
    unlink(c!("x"));
    let pid = fork_or_die();
    let mut x: u32 = if pid != 0 { 1 } else { 97 };
    for _ in 0..100 {
        x = x.wrapping_mul(1103515245).wrapping_add(12345);
        match x % 3 {
            0 => { close(open(c!("x"), O_RDWR | O_CREATE)); }
            1 => { link(c!("cat"), c!("x")); }
            _ => { unlink(c!("x")); }
        }
    }
    if pid != 0 { wait(null_mut()); } else { exit(0); }
    let _ = s;
}

unsafe fn subdir(s: *const c_char) {
    unlink(c!("ff"));
    if mkdir(c!("dd")) != 0 { diet!(s, "mkdir dd failed\n"); }

    let fd = open(c!("dd/ff"), O_CREATE | O_RDWR);
    if fd < 0 { diet!(s, "create dd/ff failed\n"); }
    write(fd, c!("ff") as *const c_void, 2);
    close(fd);

    if unlink(c!("dd")) >= 0 { diet!(s, "unlink dd (non-empty dir) succeeded!\n"); }
    if mkdir(c!("/dd/dd")) != 0 { diet!(s, "subdir mkdir dd/dd failed\n"); }

    let fd = open(c!("dd/dd/ff"), O_CREATE | O_RDWR);
    if fd < 0 { diet!(s, "create dd/dd/ff failed\n"); }
    write(fd, c!("FF") as *const c_void, 2);
    close(fd);

    let fd = open(c!("dd/dd/../ff"), 0);
    if fd < 0 { diet!(s, "open dd/dd/../ff failed\n"); }
    let cc = read(fd, buf_ptr(), BUFSZ as c_int);
    if cc != 2 || buf_get(0) != b'f' { diet!(s, "dd/dd/../ff wrong content\n"); }
    close(fd);

    if link(c!("dd/dd/ff"), c!("dd/dd/ffff")) != 0 { diet!(s, "link dd/dd/ff dd/dd/ffff failed\n"); }
    if unlink(c!("dd/dd/ff")) != 0 { diet!(s, "unlink dd/dd/ff failed\n"); }
    if open(c!("dd/dd/ff"), O_RDONLY) >= 0 { diet!(s, "open (unlinked) dd/dd/ff succeeded\n"); }

    if chdir(c!("dd")) != 0 { diet!(s, "chdir dd failed\n"); }
    if chdir(c!("dd/../../dd")) != 0 { diet!(s, "chdir dd/../../dd failed\n"); }
    if chdir(c!("dd/../../../dd")) != 0 { diet!(s, "chdir dd/../../../dd failed\n"); }
    if chdir(c!("./..")) != 0 { diet!(s, "chdir ./.. failed\n"); }

    let fd = open(c!("dd/dd/ffff"), 0);
    if fd < 0 { diet!(s, "open dd/dd/ffff failed\n"); }
    if read(fd, buf_ptr(), BUFSZ as c_int) != 2 { diet!(s, "read dd/dd/ffff wrong len\n"); }
    close(fd);

    if open(c!("dd/dd/ff"), O_RDONLY) >= 0 { diet!(s, "open (unlinked) dd/dd/ff succeeded!\n"); }
    if open(c!("dd/ff/ff"), O_CREATE | O_RDWR) >= 0 { diet!(s, "create dd/ff/ff succeeded!\n"); }
    if open(c!("dd/xx/ff"), O_CREATE | O_RDWR) >= 0 { diet!(s, "create dd/xx/ff succeeded!\n"); }
    if open(c!("dd"), O_CREATE) >= 0 { diet!(s, "create dd succeeded!\n"); }
    if open(c!("dd"), O_RDWR) >= 0 { diet!(s, "open dd rdwr succeeded!\n"); }
    if open(c!("dd"), O_WRONLY) >= 0 { diet!(s, "open dd wronly succeeded!\n"); }
    if link(c!("dd/ff/ff"), c!("dd/dd/xx")) == 0 { diet!(s, "link dd/ff/ff dd/dd/xx succeeded!\n"); }
    if link(c!("dd/xx/ff"), c!("dd/dd/xx")) == 0 { diet!(s, "link dd/xx/ff dd/dd/xx succeeded!\n"); }
    if link(c!("dd/ff"), c!("dd/dd/ffff")) == 0 { diet!(s, "link dd/ff dd/dd/ffff succeeded!\n"); }
    if mkdir(c!("dd/ff/ff")) == 0 { diet!(s, "mkdir dd/ff/ff succeeded!\n"); }
    if mkdir(c!("dd/xx/ff")) == 0 { diet!(s, "mkdir dd/xx/ff succeeded!\n"); }
    if mkdir(c!("dd/dd/ffff")) == 0 { diet!(s, "mkdir dd/dd/ffff succeeded!\n"); }
    if unlink(c!("dd/xx/ff")) == 0 { diet!(s, "unlink dd/xx/ff succeeded!\n"); }
    if unlink(c!("dd/ff/ff")) == 0 { diet!(s, "unlink dd/ff/ff succeeded!\n"); }
    if chdir(c!("dd/ff")) == 0 { diet!(s, "chdir dd/ff succeeded!\n"); }
    if chdir(c!("dd/xx")) == 0 { diet!(s, "chdir dd/xx succeeded!\n"); }

    if unlink(c!("dd/dd/ffff")) != 0 { diet!(s, "unlink dd/dd/ff failed\n"); }
    if unlink(c!("dd/ff")) != 0 { diet!(s, "unlink dd/ff failed\n"); }
    if unlink(c!("dd")) == 0 { diet!(s, "unlink non-empty dd succeeded!\n"); }
    if unlink(c!("dd/dd")) < 0 { diet!(s, "unlink dd/dd failed\n"); }
    if unlink(c!("dd")) < 0 { diet!(s, "unlink dd failed\n"); }
}

unsafe fn bigwrite(s: *const c_char) {
    unlink(c!("bigwrite"));
    let mut sz: c_int = 499;
    while sz < ((MAXOPBLOCKS + 2) * BSIZE) as c_int {
        let fd = open(c!("bigwrite"), O_CREATE | O_RDWR);
        if fd < 0 { diet!(s, "cannot create bigwrite\n"); }
        for _ in 0..2 {
            let cc = write(fd, buf_const_ptr(), sz);
            if cc != sz { diet!(s, "write(%d) ret %d\n", sz as u64, cc as u64); }
        }
        close(fd);
        unlink(c!("bigwrite"));
        sz += 471;
    }
}

unsafe fn bigfile(s: *const c_char) {
    const N: c_int = 20;
    const SZ: c_int = 600;
    unlink(c!("bigfile.dat"));
    let fd = open(c!("bigfile.dat"), O_CREATE | O_RDWR);
    if fd < 0 { diet!(s, "cannot create bigfile"); }
    for i in 0..N {
        for j in 0..(SZ as usize) { buf_set(j, i as u8); }
        if write(fd, buf_const_ptr(), SZ) != SZ { diet!(s, "write bigfile failed\n"); }
    }
    close(fd);

    let fd = open(c!("bigfile.dat"), 0);
    if fd < 0 { diet!(s, "cannot open bigfile\n"); }
    let mut total: c_int = 0;
    let mut i: c_int = 0;
    loop {
        let cc = read(fd, buf_ptr(), SZ / 2);
        if cc < 0 { diet!(s, "read bigfile failed\n"); }
        if cc == 0 { break; }
        if cc != SZ / 2 { diet!(s, "short read bigfile\n"); }
        if buf_get(0) != (i / 2) as u8 || buf_get((SZ / 2 - 1) as usize) != (i / 2) as u8 {
            diet!(s, "read bigfile wrong data\n");
        }
        total += cc;
        i += 1;
    }
    close(fd);
    if total != N * SZ { diet!(s, "read bigfile wrong total\n"); }
    unlink(c!("bigfile.dat"));
}

unsafe fn fourteen(s: *const c_char) {
    if mkdir(c!("12345678901234")) != 0 { diet!(s, "mkdir 12345678901234 failed\n"); }
    if mkdir(c!("12345678901234/123456789012345")) != 0 {
        diet!(s, "mkdir 12345678901234/123456789012345 failed\n");
    }
    let fd = open(c!("123456789012345/123456789012345/123456789012345"), O_CREATE);
    if fd < 0 { diet!(s, "create 123456789012345/123456789012345/123456789012345 failed\n"); }
    close(fd);
    let fd = open(c!("12345678901234/12345678901234/12345678901234"), 0);
    if fd < 0 { diet!(s, "open 12345678901234/12345678901234/12345678901234 failed\n"); }
    close(fd);

    if mkdir(c!("12345678901234/12345678901234")) == 0 {
        diet!(s, "mkdir 12345678901234/12345678901234 succeeded!\n");
    }
    if mkdir(c!("123456789012345/12345678901234")) == 0 {
        diet!(s, "mkdir 12345678901234/123456789012345 succeeded!\n");
    }

    unlink(c!("123456789012345/12345678901234"));
    unlink(c!("12345678901234/12345678901234"));
    unlink(c!("12345678901234/12345678901234/12345678901234"));
    unlink(c!("123456789012345/123456789012345/123456789012345"));
    unlink(c!("12345678901234/123456789012345"));
    unlink(c!("12345678901234"));
}

unsafe fn rmdot(s: *const c_char) {
    if mkdir(c!("dots")) != 0 { diet!(s, "mkdir dots failed\n"); }
    if chdir(c!("dots")) != 0 { diet!(s, "chdir dots failed\n"); }
    if unlink(c!(".")) == 0 { diet!(s, "rm . worked!\n"); }
    if unlink(c!("..")) == 0 { diet!(s, "rm .. worked!\n"); }
    if chdir(c!("/")) != 0 { diet!(s, "chdir / failed\n"); }
    if unlink(c!("dots/.")) == 0 { diet!(s, "unlink dots/. worked!\n"); }
    if unlink(c!("dots/..")) == 0 { diet!(s, "unlink dots/.. worked!\n"); }
    if unlink(c!("dots")) != 0 { diet!(s, "unlink dots failed!\n"); }
}

unsafe fn dirfile(s: *const c_char) {
    let fd = open(c!("dirfile"), O_CREATE);
    if fd < 0 { diet!(s, "create dirfile failed\n"); }
    close(fd);
    if chdir(c!("dirfile")) == 0 { diet!(s, "chdir dirfile succeeded!\n"); }
    if open(c!("dirfile/xx"), 0) >= 0 { diet!(s, "create dirfile/xx succeeded!\n"); }
    if open(c!("dirfile/xx"), O_CREATE) >= 0 { diet!(s, "create dirfile/xx succeeded!\n"); }
    if mkdir(c!("dirfile/xx")) == 0 { diet!(s, "mkdir dirfile/xx succeeded!\n"); }
    if unlink(c!("dirfile/xx")) == 0 { diet!(s, "unlink dirfile/xx succeeded!\n"); }
    if link(c!("README"), c!("dirfile/xx")) == 0 { diet!(s, "link to dirfile/xx succeeded!\n"); }
    if unlink(c!("dirfile")) != 0 { diet!(s, "unlink dirfile failed!\n"); }

    if open(c!("."), O_RDWR) >= 0 { diet!(s, "open . for writing succeeded!\n"); }
    let fd = open(c!("."), 0);
    if write(fd, c!("x") as *const c_void, 1) > 0 { diet!(s, "write . succeeded!\n"); }
    close(fd);
}

unsafe fn iref(s: *const c_char) {
    for _ in 0..(NINODE + 1) {
        if mkdir(c!("irefd")) != 0 { diet!(s, "mkdir irefd failed\n"); }
        if chdir(c!("irefd")) != 0 { diet!(s, "chdir irefd failed\n"); }
        mkdir(c!(""));
        link(c!("README"), c!(""));
        let fd = open(c!(""), O_CREATE);
        if fd >= 0 { close(fd); }
        let fd = open(c!("xx"), O_CREATE);
        if fd >= 0 { close(fd); }
        unlink(c!("xx"));
    }
    for _ in 0..(NINODE + 1) {
        chdir(c!(".."));
        unlink(c!("irefd"));
    }
    chdir(c!("/"));
}

unsafe fn forktest(s: *const c_char) {
    const N: c_int = 1000;
    let mut n: c_int = 0;
    while n < N {
        let pid = fork();
        if pid < 0 { break; }
        if pid == 0 { exit(0); }
        n += 1;
    }
    if n == 0 { diet!(s, "no fork at all!\n"); }
    if n == N { diet!(s, "fork claimed to work 1000 times!\n"); }
    while n > 0 {
        if wait(null_mut()) < 0 { diet!(s, "wait stopped early\n"); }
        n -= 1;
    }
    if wait(null_mut()) != -1 { diet!(s, "wait got too many\n"); }
}

unsafe fn sbrkbasic(s: *const c_char) {
    const TOOMUCH: c_int = 1024 * 1024 * 1024;
    let pid = fork();
    if pid < 0 { die!("fork failed in sbrkbasic\n"); }
    if pid == 0 {
        let a = sbrk(TOOMUCH);
        if a == SBRK_ERROR { exit(0); }
        let mut b = a;
        while (b as u64) < (a as u64 + TOOMUCH as u64) {
            *b = 99;
            b = b.add(PGSIZE as usize);
        }
        exit(1);
    }
    if wait1() == 1 { diet!(s, "too much memory allocated!\n"); }

    let mut a = sbrk(0);
    for i in 0..5000 {
        let b = sbrk(1);
        if b != a { diet!(s, "sbrk test failed %d %p %p\n", i as u64, a as u64, b as u64); }
        *b = 1;
        a = b.add(1);
    }
    let pid = fork();
    if pid < 0 { diet!(s, "sbrk test fork failed\n"); }
    sbrk(1);
    let c = sbrk(1);
    if c != a.add(1) { diet!(s, "sbrk test failed post-fork\n"); }
    if pid == 0 { exit(0); }
    exit(wait1());
}

unsafe fn sbrkmuch(s: *const c_char) {
    const BIG: u64 = 100 * 1024 * 1024;
    let oldbrk = sbrk(0);
    let a = sbrk(0);
    let amt = BIG - a as u64;
    let p = sbrk(amt as c_int);
    if p != a { diet!(s, "sbrk test failed to grow big address space; enough phys mem?\n"); }

    let lastaddr = (BIG - 1) as *mut c_char;
    *lastaddr = 99;

    let a = sbrk(0);
    if sbrk(-(PGSIZE as c_int)) == SBRK_ERROR { diet!(s, "sbrk could not deallocate\n"); }
    let c = sbrk(0);
    if c != a.sub(PGSIZE as usize) {
        diet!(s, "sbrk deallocation produced wrong address, a %p c %p\n", a as u64, c as u64);
    }

    let a = sbrk(0);
    let c = sbrk(PGSIZE as c_int);
    if c != a || sbrk(0) != a.add(PGSIZE as usize) {
        diet!(s, "sbrk re-allocation failed, a %p c %p\n", a as u64, c as u64);
    }
    if *lastaddr == 99 { diet!(s, "sbrk de-allocation didn't really deallocate\n"); }

    let a = sbrk(0);
    let diff = (a as i64) - (oldbrk as i64);
    let c = sbrk(-(diff as c_int));
    if c != a { diet!(s, "sbrk downsize failed, a %p c %p\n", a as u64, c as u64); }
}

unsafe fn kernmem(s: *const c_char) {
    let mut a: u64 = KERNBASE;
    while a < KERNBASE + 2_000_000 {
        let pid = fork_or_die();
        if pid == 0 {
            let v = core::ptr::read_volatile(a as *const u8);
            diet!(s, "oops could read %p = %x\n", a, v as u64);
        }
        if wait1() != -1 { exit(1); }
        a += 50000;
    }
}

unsafe fn MAXVAplus(s: *const c_char) {
    let mut a: u64 = MAXVA;
    while a != 0 {
        let pid = fork_or_die();
        if pid == 0 {
            core::ptr::write_volatile(a as *mut c_char, 99);
            diet!(s, "oops wrote %p\n", a);
        }
        if wait1() != -1 { exit(1); }
        a <<= 1;
    }
}

unsafe fn sbrkfail(s: *const c_char) {
    const BIG: u64 = 100 * 1024 * 1024;
    let mut pids = [0 as c_int; 10];
    let mut fds = [0 as c_int; 2];
    let mut failed = false;

    if pipe(fds.as_mut_ptr()) != 0 { diet!(s, "pipe() failed\n"); }
    for i in 0..pids.len() {
        pids[i] = fork();
        if pids[i] == 0 {
            let cur = sbrk(0) as u64;
            let m = if sbrk((BIG - cur) as c_int) == SBRK_ERROR { c!("0") } else { c!("1") };
            write(fds[1], m as *const c_void, 1);
            loop { pause(1000); }
        }
        if pids[i] != -1 {
            let mut sc: c_char = 0;
            read(fds[0], (&mut sc as *mut c_char).cast(), 1);
            if sc as u8 == b'0' { failed = true; }
        }
    }
    if !failed { pft!(s, "no allocation failed; allocate more?\n"); }

    let c = sbrk(PGSIZE as c_int);
    for i in 0..pids.len() {
        if pids[i] == -1 { continue; }
        kill(pids[i]);
        wait(null_mut());
    }
    if c == SBRK_ERROR { diet!(s, "failed sbrk leaked memory\n"); }

    let pid = fork_or_die();
    if pid == 0 {
        if sbrk((10 * BIG) as c_int) == SBRK_ERROR { exit(0); }
        diet!(s, "allocate a lot of memory succeeded %d\n", (10 * BIG) as u64);
    }
    if wait1() != 0 { exit(1); }
}

unsafe fn sbrkarg(s: *const c_char) {
    let a = sbrk(PGSIZE as c_int);
    let fd = open(c!("sbrk"), O_CREATE | O_WRONLY);
    unlink(c!("sbrk"));
    if fd < 0 { diet!(s, "open sbrk failed\n"); }
    if write(fd, a as *const c_void, PGSIZE as c_int) < 0 { diet!(s, "write sbrk failed\n"); }
    close(fd);

    let a = sbrk(PGSIZE as c_int);
    if pipe(a as *mut c_int) != 0 { diet!(s, "pipe() failed\n"); }
}

unsafe fn validatetest(s: *const c_char) {
    let hi: u64 = 1100 * 1024;
    let mut p: u64 = 0;
    while p <= hi {
        if link(c!("nosuchfile"), p as *const c_char) != -1 { diet!(s, "link should not succeed\n"); }
        p += PGSIZE;
    }
}

static mut UNINIT: [u8; 10000] = [0; 10000];
unsafe fn bsstest(s: *const c_char) {
    let p = addr_of!(UNINIT) as *const u8;
    for i in 0..10000 {
        if *p.add(i) != 0 { diet!(s, "bss test failed\n"); }
    }
}

static mut BIGARG_ARGS: [*mut c_char; MAXARG] = [null_mut(); MAXARG];
unsafe fn bigargtest(s: *const c_char) {
    unlink(c!("bigarg-ok"));
    let pid = fork();
    if pid == 0 {
        let mut big = [b' '; 400];
        big[399] = 0;
        let argsp = addr_of_mut!(BIGARG_ARGS) as *mut *mut c_char;
        for i in 0..(MAXARG - 1) { *argsp.add(i) = big.as_mut_ptr().cast(); }
        *argsp.add(MAXARG - 1) = null_mut();
        exec(c!("echo"), argsp);
        let fd = open(c!("bigarg-ok"), O_CREATE);
        close(fd);
        exit(0);
    } else if pid < 0 {
        diet!(s, "bigargtest: fork failed\n");
    }
    let xs = wait1();
    if xs != 0 { exit(xs); }
    let fd = open(c!("bigarg-ok"), 0);
    if fd < 0 { diet!(s, "bigarg test failed!\n"); }
    close(fd);
}

unsafe fn argptest(s: *const c_char) {
    let fd = open(c!("init"), O_RDONLY);
    if fd < 0 { diet!(s, "open failed\n"); }
    let p = sbrk(0).sub(1);
    read(fd, p as *mut c_void, -1);
    close(fd);
}

unsafe fn stacktest(s: *const c_char) {
    let pid = fork();
    if pid == 0 {
        let sp = (r_sp() as *mut c_char).sub((USERSTACK * PGSIZE) as usize);
        let v = core::ptr::read_volatile(sp);
        diet!(s, "stacktest: read below stack %d\n", v as u64);
    } else if pid < 0 {
        diet!(s, "fork failed\n");
    }
    let xs = wait1();
    if xs == -1 { exit(0); }
    exit(xs);
}

unsafe fn nowrite(s: *const c_char) {
    let addrs: [u64; 6] = [0, 0x80000000, 0x3fffffe000, 0x3ffffff000, 0x4000000000, !0];
    for &addr in addrs.iter() {
        let pid = fork();
        if pid == 0 {
            core::ptr::write_volatile(addr as *mut c_int, 10);
            diet!(s, "write to %p did not fail!\n", addr);
        } else if pid < 0 {
            diet!(s, "fork failed\n");
        }
        if wait1() == 0 { exit(1); }
    }
    exit(0);
}

const BIG_PGBUG: u64 = 0xeaeb0b5b00002f5e;
unsafe fn pgbug(_s: *const c_char) {
    let mut argv = [null_mut::<c_char>()];
    exec(BIG_PGBUG as *const c_char, argv.as_mut_ptr());
    pipe(BIG_PGBUG as *mut c_int);
    exit(0);
}

unsafe fn sbrkbugs(_s: *const c_char) {
    let pid = fork();
    if pid < 0 { die!("fork failed\n"); }
    if pid == 0 { let sz = sbrk(0) as u64; sbrk(-(sz as c_int)); exit(0); }
    wait(null_mut());

    let pid = fork();
    if pid < 0 { die!("fork failed\n"); }
    if pid == 0 { let sz = sbrk(0) as u64; sbrk(-((sz as c_int) - 3500)); exit(0); }
    wait(null_mut());

    let pid = fork();
    if pid < 0 { die!("fork failed\n"); }
    if pid == 0 {
        let cur = sbrk(0) as u64;
        sbrk((10 * PGSIZE as i64 + 2048 - cur as i64) as c_int);
        sbrk(-10);
        exit(0);
    }
    wait(null_mut());
    exit(0);
}

unsafe fn sbrklast(_s: *const c_char) {
    let top = sbrk(0) as u64;
    if top % PGSIZE != 0 { sbrk((PGSIZE - top % PGSIZE) as c_int); }
    sbrk(PGSIZE as c_int); sbrk(10); sbrk(-20);
    let top = sbrk(0) as u64;
    let p = (top - 64) as *mut c_char;
    *p = b'x' as c_char;
    *p.add(1) = 0;
    let fd = open(p, O_RDWR | O_CREATE);
    write(fd, p as *const c_void, 1);
    close(fd);
    let fd = open(p, O_RDWR);
    *p = 0;
    read(fd, p as *mut c_void, 1);
    if *p != b'x' as c_char { exit(1); }
}

unsafe fn sbrk8000(_s: *const c_char) {
    sbrk(0x80000004u32 as c_int);
    let p = sbrk(0).sub(1);
    let v = core::ptr::read_volatile(p);
    core::ptr::write_volatile(p, v.wrapping_add(1));
}

unsafe fn badarg(_s: *const c_char) {
    for _ in 0..50000 {
        let mut argv = [0xffffffffu64 as *mut c_char, null_mut()];
        exec(c!("echo"), argv.as_mut_ptr());
    }
    exit(0);
}

const REGION_SZ: c_int = 1024 * 1024 * 1024;

unsafe fn lazy_alloc(_s: *const c_char) {
    let prev_end = sbrklazy(REGION_SZ);
    if prev_end == SBRK_ERROR { die!("sbrklazy() failed\n"); }
    let new_end = prev_end.add(REGION_SZ as usize);

    let step = 64 * PGSIZE as usize;
    let mut i = prev_end.add(PGSIZE as usize);
    while (i as usize) < (new_end as usize) {
        *(i as *mut *mut c_char) = i;
        i = i.add(step);
    }
    let mut i = prev_end.add(PGSIZE as usize);
    while (i as usize) < (new_end as usize) {
        if *(i as *mut *mut c_char) != i { die!("failed to read value from memory\n"); }
        i = i.add(step);
    }
    exit(0);
}

unsafe fn lazy_unmap(_s: *const c_char) {
    let prev_end = sbrklazy(REGION_SZ);
    if prev_end == SBRK_ERROR { die!("sbrklazy() failed\n"); }
    let new_end = prev_end.add(REGION_SZ as usize);

    let step = (PGSIZE * PGSIZE) as usize;
    let mut i = prev_end.add(PGSIZE as usize);
    while (i as usize) < (new_end as usize) {
        *(i as *mut *mut c_char) = i;
        i = i.add(step);
    }

    let mut i = prev_end.add(PGSIZE as usize);
    while (i as usize) < (new_end as usize) {
        let pid = fork();
        if pid < 0 { die!("error forking\n"); }
        if pid == 0 {
            sbrklazy(-REGION_SZ);
            *(i as *mut *mut c_char) = i;
            exit(0);
        }
        if wait1() == 0 { die!("memory not unmapped\n"); }
        i = i.add(step);
    }
    exit(0);
}

unsafe fn lazy_copy(_s: *const c_char) {
    {
        let p = sbrk(0);
        sbrklazy(4 * PGSIZE as c_int);
        open(p.add(8192) as *const c_char, 0);
    }
    {
        let xx = sbrk(0);
        let ret = sbrk(-((xx as c_int) + 1));
        if ret != xx { die!("sbrk(sbrk(0)+1) returned %p, not old sz\n", ret as u64); }
    }

    let bad: [u64; 6] = [0x3fffffc000, 0x3fffffd000, 0x3fffffe000, 0x3ffffff000, 0x4000000000, 0x8000000000];
    for &b in bad.iter() {
        let fd = open(c!("README"), 0);
        if fd < 0 { die!("cannot open README\n"); }
        if read(fd, b as *mut c_void, 512) >= 0 { die!("read succeeded\n"); }
        close(fd);
        let fd = open(c!("junk"), O_CREATE | O_RDWR | O_TRUNC);
        if fd < 0 { die!("cannot open junk\n"); }
        if write(fd, b as *const c_void, 512) >= 0 { die!("write succeeded\n"); }
        close(fd);
    }
    exit(0);
}

unsafe fn lazy_sbrk(_s: *const c_char) {
    let mut p = sbrk(0);
    while (p as u64) < MAXVA - (1u64 << 30) {
        p = sbrklazy(1 << 30);
        if (p as i64) < 0 { die!("sbrklazy(%d) returned %p\n", 1u64 << 30, p as u64); }
        p = sbrklazy(0);
    }

    let n = (TRAPFRAME - PGSIZE - p as u64) as c_int;
    let p1 = sbrklazy(n);
    if (p1 as i64) < 0 || p1 != p {
        die!("sbrklazy(%d) returned %p, not expected %p\n", n as u64, p1 as u64, p as u64);
    }

    let p = sbrk(PGSIZE as c_int);
    if (p as i64) < 0 || (p as u64) != TRAPFRAME - PGSIZE {
        die!("sbrk(%d) returned %p, not expected TRAPFRAME-PGSIZE\n", PGSIZE, p as u64);
    }
    *p = 1;
    if *p.add(1) != 0 { die!("sbrk() returned non-zero-filled memory\n"); }

    let p = sbrk(1);
    if (p as i64) != -1 { die!("sbrk(1) returned %p, expected error\n", p as u64); }
    let p = sbrklazy(1);
    if (p as i64) != -1 { die!("sbrklazy(1) returned %p, expected error\n", p as u64); }
    exit(0);
}

unsafe fn bigdir(s: *const c_char) {
    const N: c_int = 500;
    let mut name = [0u8; 10];
    unlink(c!("bd"));
    let fd = open(c!("bd"), O_CREATE);
    if fd < 0 { diet!(s, "bigdir create failed\n"); }
    close(fd);

    for i in 0..N {
        name[0] = b'x';
        name[1] = b'0' + (i / 64) as u8;
        name[2] = b'0' + (i % 64) as u8;
        name[3] = 0;
        if link(c!("bd"), name.as_ptr().cast()) != 0 {
            diet!(s, "bigdir i=%d link(bd, %s) failed\n", i as u64, name.as_ptr() as u64);
        }
    }
    unlink(c!("bd"));
    for i in 0..N {
        name[1] = b'0' + (i / 64) as u8;
        name[2] = b'0' + (i % 64) as u8;
        if unlink(name.as_ptr().cast()) != 0 { diet!(s, "bigdir unlink failed"); }
    }
}

unsafe fn manywrites(s: *const c_char) {
    let nchildren: c_int = 4;
    let howmany: c_int = 30;
    for ci in 0..nchildren {
        let pid = fork();
        if pid < 0 { die!("fork failed\n"); }
        if pid == 0 {
            let name = [b'b', b'a' + ci as u8, 0];
            unlink(name.as_ptr().cast());
            for _ in 0..howmany {
                for _ in 0..(ci + 1) {
                    let fd = open(name.as_ptr().cast(), O_CREATE | O_RDWR);
                    if fd < 0 { diet!(s, "cannot create %s\n", name.as_ptr() as u64); }
                    let sz = BUFSZ as c_int;
                    let cc = write(fd, buf_const_ptr(), sz);
                    if cc != sz { diet!(s, "write(%d) ret %d\n", sz as u64, cc as u64); }
                    close(fd);
                }
                unlink(name.as_ptr().cast());
            }
            unlink(name.as_ptr().cast());
            exit(0);
        }
    }
    for _ in 0..nchildren {
        let st = wait1();
        if st != 0 { exit(st); }
    }
    exit(0);
}

unsafe fn badwrite(_s: *const c_char) {
    let assumed_free: c_int = 600;
    unlink(c!("junk"));
    for _ in 0..assumed_free {
        let fd = open(c!("junk"), O_CREATE | O_WRONLY);
        if fd < 0 { die!("open junk failed\n"); }
        write(fd, 0xffffffffffu64 as *const c_void, 1);
        close(fd);
        unlink(c!("junk"));
    }
    let fd = open(c!("junk"), O_CREATE | O_WRONLY);
    if fd < 0 { die!("open junk failed\n"); }
    if write(fd, c!("x") as *const c_void, 1) != 1 { die!("write failed\n"); }
    close(fd);
    unlink(c!("junk"));
    exit(0);
}

unsafe fn execout(_s: *const c_char) {
    for avail in 0..15 {
        let pid = fork();
        if pid < 0 { die!("fork failed\n"); }
        if pid == 0 {
            loop {
                let a = sbrk(PGSIZE as c_int);
                if a == SBRK_ERROR { break; }
                *a.add((PGSIZE - 1) as usize) = 1;
            }
            for _ in 0..avail { sbrk(-(PGSIZE as c_int)); }
            close(1);
            let mut args = [c!("echo") as *mut c_char, c!("x") as *mut c_char, null_mut()];
            exec(c!("echo"), args.as_mut_ptr());
            exit(0);
        }
        wait(null_mut());
    }
    exit(0);
}

unsafe fn diskfull(s: *const c_char) {
    let mut done = false;
    unlink(c!("diskfulldir"));

    let mut fi: c_int = 0;
    while !done && b'0' as c_int + fi < 0o177 {
        let mut name = [0u8; 32];
        name[..4].copy_from_slice(b"big0");
        name[3] = b'0' + fi as u8;
        unlink(name.as_ptr().cast());
        let fd = open(name.as_ptr().cast(), O_CREATE | O_RDWR | O_TRUNC);
        if fd < 0 { diet!(s, "could not create file %s\n", name.as_ptr() as u64); }
        for _ in 0..MAXFILE {
            let local = [0u8; BSIZE];
            if write(fd, local.as_ptr().cast(), BSIZE as c_int) != BSIZE as c_int {
                done = true;
                break;
            }
        }
        close(fd);
        fi += 1;
    }

    let nzz: c_int = 128;
    for i in 0..nzz {
        let mut name = [0u8; 32];
        name[..4].copy_from_slice(b"zz00");
        name[2] = b'0' + (i / 32) as u8;
        name[3] = b'0' + (i % 32) as u8;
        unlink(name.as_ptr().cast());
        let fd = open(name.as_ptr().cast(), O_CREATE | O_RDWR | O_TRUNC);
        if fd < 0 { break; }
        close(fd);
    }

    if mkdir(c!("diskfulldir")) == 0 { pft!(s, "mkdir(diskfulldir) unexpectedly succeeded!\n"); }
    unlink(c!("diskfulldir"));

    for i in 0..nzz {
        let mut name = [0u8; 32];
        name[..4].copy_from_slice(b"zz00");
        name[2] = b'0' + (i / 32) as u8;
        name[3] = b'0' + (i % 32) as u8;
        unlink(name.as_ptr().cast());
    }
    let mut i: c_int = 0;
    while b'0' as c_int + i < 0o177 {
        let mut name = [0u8; 32];
        name[..4].copy_from_slice(b"big0");
        name[3] = b'0' + i as u8;
        unlink(name.as_ptr().cast());
        i += 1;
    }
}

unsafe fn outofinodes(_s: *const c_char) {
    let nzz: c_int = 32 * 32;
    for i in 0..nzz {
        let mut name = [0u8; 32];
        name[..4].copy_from_slice(b"zz00");
        name[2] = b'0' + (i / 32) as u8;
        name[3] = b'0' + (i % 32) as u8;
        unlink(name.as_ptr().cast());
        let fd = open(name.as_ptr().cast(), O_CREATE | O_RDWR | O_TRUNC);
        if fd < 0 { break; }
        close(fd);
    }
    for i in 0..nzz {
        let mut name = [0u8; 32];
        name[..4].copy_from_slice(b"zz00");
        name[2] = b'0' + (i / 32) as u8;
        name[3] = b'0' + (i % 32) as u8;
        unlink(name.as_ptr().cast());
    }
}

// ============== test runner ==============

struct Test {
    f: unsafe fn(*const c_char),
    s: &'static [u8],
}

static QUICKTESTS: &[Test] = &[
    t!("copyin", copyin),
    t!("copyout", copyout),
    t!("copyinstr1", copyinstr1),
    t!("copyinstr2", copyinstr2),
    t!("copyinstr3", copyinstr3),
    t!("rwsbrk", rwsbrk),
    t!("truncate1", truncate1),
    t!("truncate2", truncate2),
    t!("truncate3", truncate3),
    t!("openiput", openiput),
    t!("exitiput", exitiput),
    t!("iput", iputtest),
    t!("opentest", opentest),
    t!("writetest", writetest),
    t!("writebig", writebig),
    t!("createtest", createtest),
    t!("dirtest", dirtest),
    t!("exectest", exectest),
    t!("pipe1", pipe1),
    t!("killstatus", killstatus),
    t!("preempt", preempt),
    t!("exitwait", exitwait),
    t!("reparent", reparent),
    t!("twochildren", twochildren),
    t!("forkfork", forkfork),
    t!("forkforkfork", forkforkfork),
    t!("reparent2", reparent2),
    t!("mem", mem),
    t!("sharedfd", sharedfd),
    t!("fourfiles", fourfiles),
    t!("createdelete", createdelete),
    t!("unlinkread", unlinkread),
    t!("linktest", linktest),
    t!("concreate", concreate),
    t!("linkunlink", linkunlink),
    t!("subdir", subdir),
    t!("bigwrite", bigwrite),
    t!("bigfile", bigfile),
    t!("fourteen", fourteen),
    t!("rmdot", rmdot),
    t!("dirfile", dirfile),
    t!("iref", iref),
    t!("forktest", forktest),
    t!("sbrkbasic", sbrkbasic),
    t!("sbrkmuch", sbrkmuch),
    t!("kernmem", kernmem),
    t!("MAXVAplus", MAXVAplus),
    t!("sbrkfail", sbrkfail),
    t!("sbrkarg", sbrkarg),
    t!("validatetest", validatetest),
    t!("bsstest", bsstest),
    t!("bigargtest", bigargtest),
    t!("argptest", argptest),
    t!("stacktest", stacktest),
    t!("nowrite", nowrite),
    t!("pgbug", pgbug),
    t!("sbrkbugs", sbrkbugs),
    t!("sbrklast", sbrklast),
    t!("sbrk8000", sbrk8000),
    t!("badarg", badarg),
    t!("lazy_alloc", lazy_alloc),
    t!("lazy_unmap", lazy_unmap),
    t!("lazy_copy", lazy_copy),
    t!("lazy_sbrk", lazy_sbrk),
];

static SLOWTESTS: &[Test] = &[
    t!("bigdir", bigdir),
    t!("manywrites", manywrites),
    t!("badwrite", badwrite),
    t!("execout", execout),
    t!("diskfull", diskfull),
    t!("outofinodes", outofinodes),
];

unsafe fn run_one(f: unsafe fn(*const c_char), s: *const c_char) -> bool {
    pf!("test %s: ", s as u64);
    let pid = fork();
    if pid < 0 { die!("runtest: fork error\n"); }
    if pid == 0 { f(s); exit(0); }
    let xs = wait1();
    if xs != 0 { pf!("FAILED\n"); } else { pf!("OK\n"); }
    xs == 0
}

unsafe fn runtests(tests: &[Test], justone: *const c_char, continuous: c_int) -> c_int {
    let mut n: c_int = 0;
    for t in tests {
        let s = t.s.as_ptr() as *const c_char;
        if justone.is_null() || strcmp(s, justone) == 0 {
            n += 1;
            if !run_one(t.f, s) && continuous != 2 {
                pf!("SOME TESTS FAILED\n");
                return -1;
            }
        }
    }
    n
}

unsafe fn countfree() -> c_int {
    let sz0 = sbrk(0) as u64;
    let mut n: c_int = 0;
    while sbrk(PGSIZE as c_int) != SBRK_ERROR { n += 1; }
    sbrk(-((sbrk(0) as u64 - sz0) as c_int));
    n
}

unsafe fn drivetests(quick: bool, continuous: c_int, justone: *const c_char) -> c_int {
    loop {
        pf!("usertests starting\n");
        let free0 = countfree();
        let mut total: c_int = 0;

        let n = runtests(QUICKTESTS, justone, continuous);
        if n < 0 { if continuous != 2 { return 1; } } else { total += n; }

        if !quick {
            if justone.is_null() { pf!("usertests slow tests starting\n"); }
            let n = runtests(SLOWTESTS, justone, continuous);
            if n < 0 { if continuous != 2 { return 1; } } else { total += n; }
        }

        let free1 = countfree();
        if free1 < free0 {
            pf!("FAILED -- lost some free pages %d (out of %d)\n", free1 as u64, free0 as u64);
            if continuous != 2 { return 1; }
        }
        if !justone.is_null() && total == 0 { pf!("NO TESTS EXECUTED\n"); return 1; }
        if continuous == 0 { return 0; }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn main(argc: c_int, argv: *mut *mut c_char) -> c_int {
    let mut continuous: c_int = 0;
    let mut quick = false;
    let mut justone: *const c_char = core::ptr::null();

    if argc == 2 {
        let a = argv_at(argv, 1);
        if strcmp(a, c!("-q")) == 0 { quick = true; }
        else if strcmp(a, c!("-c")) == 0 { continuous = 1; }
        else if strcmp(a, c!("-C")) == 0 { continuous = 2; }
        else if *a != b'-' as c_char { justone = a; }
        else { die!("Usage: usertests [-c] [-C] [-q] [testname]\n"); }
    } else if argc > 1 {
        die!("Usage: usertests [-c] [-C] [-q] [testname]\n");
    }
    if drivetests(quick, continuous, justone) != 0 { exit(1); }
    pf!("ALL TESTS PASSED\n");
    exit(0);
}
