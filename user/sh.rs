#![no_std]
#![allow(dead_code)]

use core::ffi::{c_char, c_int, c_uint, c_void};
use core::mem::size_of;

mod rust_user;
use rust_user::*;

unsafe extern "C" {
    fn gets(buf: *mut c_char, max: c_int) -> *mut c_char;
    fn malloc(n: c_uint) -> *mut c_void;
    fn free(p: *mut c_void);
}

// Parsed command representation
const EXEC: c_int = 1;
const REDIR: c_int = 2;
const PIPE: c_int = 3;
const LIST: c_int = 4;
const BACK: c_int = 5;

const MAXARGS: usize = 10;

#[repr(C)]
struct Cmd {
    cmd_type: c_int,
}

#[repr(C)]
struct ExecCmd {
    cmd_type: c_int,
    argv: [*mut c_char; MAXARGS],
    eargv: [*mut c_char; MAXARGS],
}

#[repr(C)]
struct RedirCmd {
    cmd_type: c_int,
    cmd: *mut Cmd,
    file: *mut c_char,
    efile: *mut c_char,
    mode: c_int,
    fd: c_int,
}

#[repr(C)]
struct PipeCmd {
    cmd_type: c_int,
    left: *mut Cmd,
    right: *mut Cmd,
}

#[repr(C)]
struct ListCmd {
    cmd_type: c_int,
    left: *mut Cmd,
    right: *mut Cmd,
}

#[repr(C)]
struct BackCmd {
    cmd_type: c_int,
    cmd: *mut Cmd,
}

unsafe fn panic_msg(s: *const c_char) -> ! {
    fprintf(2, b"%s\n\0".as_ptr().cast(), s as u64);
    exit(1);
}

unsafe fn fork1() -> c_int {
    let pid = fork();
    if pid == -1 {
        panic_msg(b"fork\0".as_ptr().cast());
    }
    pid
}

// Execute cmd. Never returns.
unsafe fn runcmd(cmd: *mut Cmd) -> ! {
    let mut p: [c_int; 2] = [0; 2];

    if cmd.is_null() {
        exit(1);
    }

    match (*cmd).cmd_type {
        EXEC => {
            let ecmd = cmd as *mut ExecCmd;
            if (*ecmd).argv[0].is_null() {
                exit(1);
            }
            exec((*ecmd).argv[0], (*ecmd).argv.as_mut_ptr());
            fprintf(
                2,
                b"exec %s failed\n\0".as_ptr().cast(),
                (*ecmd).argv[0] as u64,
            );
        }
        REDIR => {
            let rcmd = cmd as *mut RedirCmd;
            close((*rcmd).fd);
            if open((*rcmd).file, (*rcmd).mode) < 0 {
                fprintf(2, b"open %s failed\n\0".as_ptr().cast(), (*rcmd).file as u64);
                exit(1);
            }
            runcmd((*rcmd).cmd);
        }
        LIST => {
            let lcmd = cmd as *mut ListCmd;
            if fork1() == 0 {
                runcmd((*lcmd).left);
            }
            wait(core::ptr::null_mut());
            runcmd((*lcmd).right);
        }
        PIPE => {
            let pcmd = cmd as *mut PipeCmd;
            if pipe(p.as_mut_ptr()) < 0 {
                panic_msg(b"pipe\0".as_ptr().cast());
            }
            if fork1() == 0 {
                close(1);
                dup(p[1]);
                close(p[0]);
                close(p[1]);
                runcmd((*pcmd).left);
            }
            if fork1() == 0 {
                close(0);
                dup(p[0]);
                close(p[0]);
                close(p[1]);
                runcmd((*pcmd).right);
            }
            close(p[0]);
            close(p[1]);
            wait(core::ptr::null_mut());
            wait(core::ptr::null_mut());
        }
        BACK => {
            let bcmd = cmd as *mut BackCmd;
            if fork1() == 0 {
                runcmd((*bcmd).cmd);
            }
        }
        _ => {
            panic_msg(b"runcmd\0".as_ptr().cast());
        }
    }
    exit(0);
}

unsafe fn getcmd(buf: *mut c_char, nbuf: c_int) -> c_int {
    write(2, b"$ \0".as_ptr().cast(), 2);
    memset(buf.cast::<c_void>(), 0, nbuf as c_uint);
    gets(buf, nbuf);
    if *buf == 0 {
        return -1;
    }
    0
}

// Constructors
unsafe fn execcmd() -> *mut Cmd {
    let cmd = malloc(size_of::<ExecCmd>() as c_uint) as *mut ExecCmd;
    memset(cmd.cast::<c_void>(), 0, size_of::<ExecCmd>() as c_uint);
    (*cmd).cmd_type = EXEC;
    cmd as *mut Cmd
}

unsafe fn redircmd(
    subcmd: *mut Cmd,
    file: *mut c_char,
    efile: *mut c_char,
    mode: c_int,
    fd: c_int,
) -> *mut Cmd {
    let cmd = malloc(size_of::<RedirCmd>() as c_uint) as *mut RedirCmd;
    memset(cmd.cast::<c_void>(), 0, size_of::<RedirCmd>() as c_uint);
    (*cmd).cmd_type = REDIR;
    (*cmd).cmd = subcmd;
    (*cmd).file = file;
    (*cmd).efile = efile;
    (*cmd).mode = mode;
    (*cmd).fd = fd;
    cmd as *mut Cmd
}

unsafe fn pipecmd(left: *mut Cmd, right: *mut Cmd) -> *mut Cmd {
    let cmd = malloc(size_of::<PipeCmd>() as c_uint) as *mut PipeCmd;
    memset(cmd.cast::<c_void>(), 0, size_of::<PipeCmd>() as c_uint);
    (*cmd).cmd_type = PIPE;
    (*cmd).left = left;
    (*cmd).right = right;
    cmd as *mut Cmd
}

unsafe fn listcmd(left: *mut Cmd, right: *mut Cmd) -> *mut Cmd {
    let cmd = malloc(size_of::<ListCmd>() as c_uint) as *mut ListCmd;
    memset(cmd.cast::<c_void>(), 0, size_of::<ListCmd>() as c_uint);
    (*cmd).cmd_type = LIST;
    (*cmd).left = left;
    (*cmd).right = right;
    cmd as *mut Cmd
}

unsafe fn backcmd(subcmd: *mut Cmd) -> *mut Cmd {
    let cmd = malloc(size_of::<BackCmd>() as c_uint) as *mut BackCmd;
    memset(cmd.cast::<c_void>(), 0, size_of::<BackCmd>() as c_uint);
    (*cmd).cmd_type = BACK;
    (*cmd).cmd = subcmd;
    cmd as *mut Cmd
}

// Parsing
static WHITESPACE: &[u8] = b" \t\r\n\x0b\0";
static SYMBOLS: &[u8] = b"<|>&;()\0";

unsafe fn gettoken(
    ps: *mut *mut c_char,
    es: *mut c_char,
    q: *mut *mut c_char,
    eq: *mut *mut c_char,
) -> c_int {
    let mut s = *ps;
    while (s as usize) < (es as usize) && !strchr(WHITESPACE.as_ptr().cast(), *s).is_null() {
        s = s.add(1);
    }
    if !q.is_null() {
        *q = s;
    }
    let mut ret: c_int = *s as c_int;
    match *s as u8 {
        0 => {}
        b'|' | b'(' | b')' | b';' | b'&' | b'<' => {
            s = s.add(1);
        }
        b'>' => {
            s = s.add(1);
            if *s == b'>' as c_char {
                ret = b'+' as c_int;
                s = s.add(1);
            }
        }
        _ => {
            ret = b'a' as c_int;
            while (s as usize) < (es as usize)
                && strchr(WHITESPACE.as_ptr().cast(), *s).is_null()
                && strchr(SYMBOLS.as_ptr().cast(), *s).is_null()
            {
                s = s.add(1);
            }
        }
    }
    if !eq.is_null() {
        *eq = s;
    }

    while (s as usize) < (es as usize) && !strchr(WHITESPACE.as_ptr().cast(), *s).is_null() {
        s = s.add(1);
    }
    *ps = s;
    ret
}

unsafe fn peek(ps: *mut *mut c_char, es: *mut c_char, toks: *const c_char) -> c_int {
    let mut s = *ps;
    while (s as usize) < (es as usize) && !strchr(WHITESPACE.as_ptr().cast(), *s).is_null() {
        s = s.add(1);
    }
    *ps = s;
    if *s != 0 && !strchr(toks, *s).is_null() {
        1
    } else {
        0
    }
}

unsafe fn parsecmd(s: *mut c_char) -> *mut Cmd {
    let es = s.add(strlen(s) as usize);
    let mut sp = s;
    let cmd = parseline(&mut sp, es);
    peek(&mut sp, es, b"\0".as_ptr().cast());
    if sp != es {
        fprintf(2, b"leftovers: %s\n\0".as_ptr().cast(), sp as u64);
        panic_msg(b"syntax\0".as_ptr().cast());
    }
    nulterminate(cmd);
    cmd
}

unsafe fn parseline(ps: *mut *mut c_char, es: *mut c_char) -> *mut Cmd {
    let mut cmd = parsepipe(ps, es);
    while peek(ps, es, b"&\0".as_ptr().cast()) != 0 {
        gettoken(ps, es, core::ptr::null_mut(), core::ptr::null_mut());
        cmd = backcmd(cmd);
    }
    if peek(ps, es, b";\0".as_ptr().cast()) != 0 {
        gettoken(ps, es, core::ptr::null_mut(), core::ptr::null_mut());
        cmd = listcmd(cmd, parseline(ps, es));
    }
    cmd
}

unsafe fn parsepipe(ps: *mut *mut c_char, es: *mut c_char) -> *mut Cmd {
    let mut cmd = parseexec(ps, es);
    if peek(ps, es, b"|\0".as_ptr().cast()) != 0 {
        gettoken(ps, es, core::ptr::null_mut(), core::ptr::null_mut());
        cmd = pipecmd(cmd, parsepipe(ps, es));
    }
    cmd
}

unsafe fn parseredirs(mut cmd: *mut Cmd, ps: *mut *mut c_char, es: *mut c_char) -> *mut Cmd {
    let mut q: *mut c_char = core::ptr::null_mut();
    let mut eq: *mut c_char = core::ptr::null_mut();

    while peek(ps, es, b"<>\0".as_ptr().cast()) != 0 {
        let tok = gettoken(ps, es, core::ptr::null_mut(), core::ptr::null_mut());
        if gettoken(ps, es, &mut q, &mut eq) != b'a' as c_int {
            panic_msg(b"missing file for redirection\0".as_ptr().cast());
        }
        match tok as u8 as char {
            '<' => {
                cmd = redircmd(cmd, q, eq, O_RDONLY, 0);
            }
            '>' => {
                cmd = redircmd(cmd, q, eq, O_WRONLY | O_CREATE | O_TRUNC, 1);
            }
            '+' => {
                cmd = redircmd(cmd, q, eq, O_WRONLY | O_CREATE, 1);
            }
            _ => {}
        }
    }
    cmd
}

unsafe fn parseblock(ps: *mut *mut c_char, es: *mut c_char) -> *mut Cmd {
    if peek(ps, es, b"(\0".as_ptr().cast()) == 0 {
        panic_msg(b"parseblock\0".as_ptr().cast());
    }
    gettoken(ps, es, core::ptr::null_mut(), core::ptr::null_mut());
    let mut cmd = parseline(ps, es);
    if peek(ps, es, b")\0".as_ptr().cast()) == 0 {
        panic_msg(b"syntax - missing )\0".as_ptr().cast());
    }
    gettoken(ps, es, core::ptr::null_mut(), core::ptr::null_mut());
    cmd = parseredirs(cmd, ps, es);
    cmd
}

unsafe fn parseexec(ps: *mut *mut c_char, es: *mut c_char) -> *mut Cmd {
    if peek(ps, es, b"(\0".as_ptr().cast()) != 0 {
        return parseblock(ps, es);
    }

    let mut ret = execcmd();
    let cmd = ret as *mut ExecCmd;

    let mut argc: usize = 0;
    ret = parseredirs(ret, ps, es);
    while peek(ps, es, b"|)&;\0".as_ptr().cast()) == 0 {
        let mut q: *mut c_char = core::ptr::null_mut();
        let mut eq: *mut c_char = core::ptr::null_mut();
        let tok = gettoken(ps, es, &mut q, &mut eq);
        if tok == 0 {
            break;
        }
        if tok != b'a' as c_int {
            panic_msg(b"syntax\0".as_ptr().cast());
        }
        (*cmd).argv[argc] = q;
        (*cmd).eargv[argc] = eq;
        argc += 1;
        if argc >= MAXARGS {
            panic_msg(b"too many args\0".as_ptr().cast());
        }
        ret = parseredirs(ret, ps, es);
    }
    (*cmd).argv[argc] = core::ptr::null_mut();
    (*cmd).eargv[argc] = core::ptr::null_mut();
    ret
}

// NUL-terminate all the counted strings.
unsafe fn nulterminate(cmd: *mut Cmd) -> *mut Cmd {
    if cmd.is_null() {
        return core::ptr::null_mut();
    }

    match (*cmd).cmd_type {
        EXEC => {
            let ecmd = cmd as *mut ExecCmd;
            let argv_ptr = core::ptr::addr_of_mut!((*ecmd).argv).cast::<*mut c_char>();
            let eargv_ptr = core::ptr::addr_of_mut!((*ecmd).eargv).cast::<*mut c_char>();
            let mut i = 0;
            while !(*argv_ptr.add(i)).is_null() {
                *(*eargv_ptr.add(i)) = 0;
                i += 1;
            }
        }
        REDIR => {
            let rcmd = cmd as *mut RedirCmd;
            nulterminate((*rcmd).cmd);
            *(*rcmd).efile = 0;
        }
        PIPE => {
            let pcmd = cmd as *mut PipeCmd;
            nulterminate((*pcmd).left);
            nulterminate((*pcmd).right);
        }
        LIST => {
            let lcmd = cmd as *mut ListCmd;
            nulterminate((*lcmd).left);
            nulterminate((*lcmd).right);
        }
        BACK => {
            let bcmd = cmd as *mut BackCmd;
            nulterminate((*bcmd).cmd);
        }
        _ => {}
    }
    cmd
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn main(_argc: c_int, _argv: *mut *mut c_char) -> c_int {
    static mut BUF: [c_char; 100] = [0; 100];

    // Ensure that three file descriptors are open.
    loop {
        let fd = open(b"console\0".as_ptr().cast(), O_RDWR);
        if fd < 0 {
            break;
        }
        if fd >= 3 {
            close(fd);
            break;
        }
    }

    // Read and run input commands.
    while getcmd(
        core::ptr::addr_of_mut!(BUF).cast::<c_char>(),
        size_of::<[c_char; 100]>() as c_int,
    ) >= 0
    {
        let buf_ptr = core::ptr::addr_of_mut!(BUF).cast::<c_char>();
        let mut cmd = buf_ptr;
        while *cmd == b' ' as c_char || *cmd == b'\t' as c_char {
            cmd = cmd.add(1);
        }
        if *cmd == b'\n' as c_char {
            continue;
        }
        if *cmd == b'c' as c_char
            && *cmd.add(1) == b'd' as c_char
            && *cmd.add(2) == b' ' as c_char
        {
            // Chdir must be called by the parent, not the child.
            let len = strlen(cmd) as usize;
            *cmd.add(len - 1) = 0; // chop \n
            if chdir(cmd.add(3)) < 0 {
                fprintf(2, b"cannot cd %s\n\0".as_ptr().cast(), cmd.add(3) as u64);
            }
            continue;
        }
        if fork1() == 0 {
            runcmd(parsecmd(cmd));
        }
        wait(core::ptr::null_mut());
    }
    exit(0);
}
