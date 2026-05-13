#![no_std]

//
// run random system calls in parallel forever.
//

use core::ffi::{c_char, c_int, c_void};

mod rust_user;
use rust_user::*;

const GO_BUF_LEN : c_int = core::mem::size_of::<[u8; 999]>() as c_int;

static mut RAND_NEXT: u64 = 1;

// from FreeBSD.
unsafe fn do_rand(ctx: *mut u64) -> c_int {
    // Compute x = (7^5 * x) mod (2^31 - 1)
    let mut x: i64 = ((*ctx % 0x7ffffffe) as i64) + 1;
    let hi: i64 = x / 127773;
    let lo: i64 = x % 127773;
    x = 16807 * lo - 2836 * hi;
    if x < 0 {
        x += 0x7fffffff;
    }
    x -= 1;
    *ctx = x as u64;
    x as c_int
}

unsafe fn rand() -> c_int {
    do_rand(core::ptr::addr_of_mut!(RAND_NEXT))
}

static mut GO_BUF: [u8; 999] = [0; 999];

unsafe fn go(which_child: c_int) {
    let mut fd: c_int = -1;
    let break0 = sbrk(0);
    let mut iters: u64 = 0;

    mkdir(b"grindir\0".as_ptr().cast());
    if chdir(b"grindir\0".as_ptr().cast()) != 0 {
        printf(b"grind: chdir grindir failed\n\0".as_ptr().cast());
        exit(1);
    }
    chdir(b"/\0".as_ptr().cast());

    loop {
        iters += 1;
        if iters % 500 == 0 {
            let s: *const c_void = if which_child != 0 {
                b"B\0".as_ptr().cast()
            } else {
                b"A\0".as_ptr().cast()
            };
            write(1, s, 1);
        }
        let what = rand() % 23;
        if what == 1 {
            close(open(
                b"grindir/../a\0".as_ptr().cast(),
                O_CREATE | O_RDWR,
            ));
        } else if what == 2 {
            close(open(
                b"grindir/../grindir/../b\0".as_ptr().cast(),
                O_CREATE | O_RDWR,
            ));
        } else if what == 3 {
            unlink(b"grindir/../a\0".as_ptr().cast());
        } else if what == 4 {
            if chdir(b"grindir\0".as_ptr().cast()) != 0 {
                printf(b"grind: chdir grindir failed\n\0".as_ptr().cast());
                exit(1);
            }
            unlink(b"../b\0".as_ptr().cast());
            chdir(b"/\0".as_ptr().cast());
        } else if what == 5 {
            close(fd);
            fd = open(
                b"/grindir/../a\0".as_ptr().cast(),
                O_CREATE | O_RDWR,
            );
        } else if what == 6 {
            close(fd);
            fd = open(
                b"/./grindir/./../b\0".as_ptr().cast(),
                O_CREATE | O_RDWR,
            );
        } else if what == 7 {
            write(
                fd,
                (&raw const GO_BUF).cast::<c_void>(),
                GO_BUF_LEN
            );
        } else if what == 8 {
            read(
                fd,
                (&raw mut GO_BUF).cast::<c_void>(),
                GO_BUF_LEN
            );
        } else if what == 9 {
            mkdir(b"grindir/../a\0".as_ptr().cast());
            close(open(
                b"a/../a/./a\0".as_ptr().cast(),
                O_CREATE | O_RDWR,
            ));
            unlink(b"a/a\0".as_ptr().cast());
        } else if what == 10 {
            mkdir(b"/../b\0".as_ptr().cast());
            close(open(
                b"grindir/../b/b\0".as_ptr().cast(),
                O_CREATE | O_RDWR,
            ));
            unlink(b"b/b\0".as_ptr().cast());
        } else if what == 11 {
            unlink(b"b\0".as_ptr().cast());
            link(
                b"../grindir/./../a\0".as_ptr().cast(),
                b"../b\0".as_ptr().cast(),
            );
        } else if what == 12 {
            unlink(b"../grindir/../a\0".as_ptr().cast());
            link(
                b".././b\0".as_ptr().cast(),
                b"/grindir/../a\0".as_ptr().cast(),
            );
        } else if what == 13 {
            let pid = fork();
            if pid == 0 {
                exit(0);
            } else if pid < 0 {
                printf(b"grind: fork failed\n\0".as_ptr().cast());
                exit(1);
            }
            wait(core::ptr::null_mut());
        } else if what == 14 {
            let pid = fork();
            if pid == 0 {
                fork();
                fork();
                exit(0);
            } else if pid < 0 {
                printf(b"grind: fork failed\n\0".as_ptr().cast());
                exit(1);
            }
            wait(core::ptr::null_mut());
        } else if what == 15 {
            sbrk(6011);
        } else if what == 16 {
            let cur = sbrk(0);
            if (cur as usize) > (break0 as usize) {
                let diff = (cur as usize) - (break0 as usize);
                sbrk(-(diff as c_int));
            }
        } else if what == 17 {
            let pid = fork();
            if pid == 0 {
                close(open(b"a\0".as_ptr().cast(), O_CREATE | O_RDWR));
                exit(0);
            } else if pid < 0 {
                printf(b"grind: fork failed\n\0".as_ptr().cast());
                exit(1);
            }
            if chdir(b"../grindir/..\0".as_ptr().cast()) != 0 {
                printf(b"grind: chdir failed\n\0".as_ptr().cast());
                exit(1);
            }
            kill(pid);
            wait(core::ptr::null_mut());
        } else if what == 18 {
            let pid = fork();
            if pid == 0 {
                kill(getpid());
                exit(0);
            } else if pid < 0 {
                printf(b"grind: fork failed\n\0".as_ptr().cast());
                exit(1);
            }
            wait(core::ptr::null_mut());
        } else if what == 19 {
            let mut fds: [c_int; 2] = [0; 2];
            if pipe(fds.as_mut_ptr()) < 0 {
                printf(b"grind: pipe failed\n\0".as_ptr().cast());
                exit(1);
            }
            let pid = fork();
            if pid == 0 {
                fork();
                fork();
                if write(fds[1], b"x\0".as_ptr().cast(), 1) != 1 {
                    printf(b"grind: pipe write failed\n\0".as_ptr().cast());
                }
                let mut c: c_char = 0;
                if read(fds[0], (&mut c as *mut c_char).cast::<c_void>(), 1) != 1 {
                    printf(b"grind: pipe read failed\n\0".as_ptr().cast());
                }
                exit(0);
            } else if pid < 0 {
                printf(b"grind: fork failed\n\0".as_ptr().cast());
                exit(1);
            }
            close(fds[0]);
            close(fds[1]);
            wait(core::ptr::null_mut());
        } else if what == 20 {
            let pid = fork();
            if pid == 0 {
                unlink(b"a\0".as_ptr().cast());
                mkdir(b"a\0".as_ptr().cast());
                chdir(b"a\0".as_ptr().cast());
                unlink(b"../a\0".as_ptr().cast());
                fd = open(b"x\0".as_ptr().cast(), O_CREATE | O_RDWR);
                unlink(b"x\0".as_ptr().cast());
                exit(0);
            } else if pid < 0 {
                printf(b"grind: fork failed\n\0".as_ptr().cast());
                exit(1);
            }
            wait(core::ptr::null_mut());
        } else if what == 21 {
            unlink(b"c\0".as_ptr().cast());
            // should always succeed. check that there are free i-nodes,
            // file descriptors, blocks.
            let fd1 = open(b"c\0".as_ptr().cast(), O_CREATE | O_RDWR);
            if fd1 < 0 {
                printf(b"grind: create c failed\n\0".as_ptr().cast());
                exit(1);
            }
            if write(fd1, b"x\0".as_ptr().cast(), 1) != 1 {
                printf(b"grind: write c failed\n\0".as_ptr().cast());
                exit(1);
            }
            let mut st: Stat = Stat {
                dev: 0,
                ino: 0,
                file_type: 0,
                nlink: 0,
                size: 0,
            };
            if fstat(fd1, &mut st as *mut Stat) != 0 {
                printf(b"grind: fstat failed\n\0".as_ptr().cast());
                exit(1);
            }
            if st.size != 1 {
                printf(
                    b"grind: fstat reports wrong size %d\n\0".as_ptr().cast(),
                    st.size as u64,
                );
                exit(1);
            }
            if st.ino > 200 {
                printf(
                    b"grind: fstat reports crazy i-number %d\n\0".as_ptr().cast(),
                    st.ino as u64,
                );
                exit(1);
            }
            close(fd1);
            unlink(b"c\0".as_ptr().cast());
        } else if what == 22 {
            // echo hi | cat
            let mut aa: [c_int; 2] = [0; 2];
            let mut bb: [c_int; 2] = [0; 2];
            if pipe(aa.as_mut_ptr()) < 0 {
                fprintf(2, b"grind: pipe failed\n\0".as_ptr().cast());
                exit(1);
            }
            if pipe(bb.as_mut_ptr()) < 0 {
                fprintf(2, b"grind: pipe failed\n\0".as_ptr().cast());
                exit(1);
            }
            let pid1 = fork();
            if pid1 == 0 {
                close(bb[0]);
                close(bb[1]);
                close(aa[0]);
                close(1);
                if dup(aa[1]) != 1 {
                    fprintf(2, b"grind: dup failed\n\0".as_ptr().cast());
                    exit(1);
                }
                close(aa[1]);
                let mut args: [*mut c_char; 3] = [
                    b"echo\0".as_ptr() as *mut c_char,
                    b"hi\0".as_ptr() as *mut c_char,
                    core::ptr::null_mut(),
                ];
                exec(
                    b"grindir/../echo\0".as_ptr().cast(),
                    args.as_mut_ptr(),
                );
                fprintf(2, b"grind: echo: not found\n\0".as_ptr().cast());
                exit(2);
            } else if pid1 < 0 {
                fprintf(2, b"grind: fork failed\n\0".as_ptr().cast());
                exit(3);
            }
            let pid2 = fork();
            if pid2 == 0 {
                close(aa[1]);
                close(bb[0]);
                close(0);
                if dup(aa[0]) != 0 {
                    fprintf(2, b"grind: dup failed\n\0".as_ptr().cast());
                    exit(4);
                }
                close(aa[0]);
                close(1);
                if dup(bb[1]) != 1 {
                    fprintf(2, b"grind: dup failed\n\0".as_ptr().cast());
                    exit(5);
                }
                close(bb[1]);
                let mut args: [*mut c_char; 2] = [
                    b"cat\0".as_ptr() as *mut c_char,
                    core::ptr::null_mut(),
                ];
                exec(b"/cat\0".as_ptr().cast(), args.as_mut_ptr());
                fprintf(2, b"grind: cat: not found\n\0".as_ptr().cast());
                exit(6);
            } else if pid2 < 0 {
                fprintf(2, b"grind: fork failed\n\0".as_ptr().cast());
                exit(7);
            }
            close(aa[0]);
            close(aa[1]);
            close(bb[1]);
            let mut buf: [c_char; 4] = [0; 4];
            read(bb[0], buf.as_mut_ptr().add(0).cast::<c_void>(), 1);
            read(bb[0], buf.as_mut_ptr().add(1).cast::<c_void>(), 1);
            read(bb[0], buf.as_mut_ptr().add(2).cast::<c_void>(), 1);
            close(bb[0]);
            let mut st1: c_int = 0;
            let mut st2: c_int = 0;
            wait(&mut st1 as *mut c_int);
            wait(&mut st2 as *mut c_int);
            if st1 != 0
                || st2 != 0
                || strcmp(buf.as_ptr(), b"hi\n\0".as_ptr().cast()) != 0
            {
                printf(
                    b"grind: exec pipeline failed %d %d \"%s\"\n\0"
                        .as_ptr()
                        .cast(),
                    st1 as u64,
                    st2 as u64,
                    buf.as_ptr() as u64,
                );
                exit(1);
            }
        }
    }
}

unsafe fn iter() -> ! {
    unlink(b"a\0".as_ptr().cast());
    unlink(b"b\0".as_ptr().cast());

    let pid1 = fork();
    if pid1 < 0 {
        printf(b"grind: fork failed\n\0".as_ptr().cast());
        exit(1);
    }
    if pid1 == 0 {
        RAND_NEXT ^= 31;
        go(0);
        exit(0);
    }

    let pid2 = fork();
    if pid2 < 0 {
        printf(b"grind: fork failed\n\0".as_ptr().cast());
        exit(1);
    }
    if pid2 == 0 {
        RAND_NEXT ^= 7177;
        go(1);
        exit(0);
    }

    let mut st1: c_int = -1;
    wait(&mut st1 as *mut c_int);
    if st1 != 0 {
        kill(pid1);
        kill(pid2);
    }
    let mut st2: c_int = -1;
    wait(&mut st2 as *mut c_int);

    exit(0);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn main(_argc: c_int, _argv: *mut *mut c_char) -> c_int {
    loop {
        let pid = fork();
        if pid == 0 {
            iter();
        }
        if pid > 0 {
            wait(core::ptr::null_mut());
        }
        pause(20);
        RAND_NEXT += 1;
    }
}
