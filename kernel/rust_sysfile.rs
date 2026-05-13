use core::ffi::{c_char, c_int, c_short, c_uint, c_void};
use core::mem;
use core::ptr;

use crate::rust_exec::kexec;
use crate::rust_file::{filealloc, fileclose, filedup, fileread, filestat, filewrite, File};
use crate::rust_fs::{
    dirlink, dirlookup, ialloc, ilock, iput, iunlock, iunlockput, iupdate, itrunc, namecmp,
    nameiparent, namei, readi, writei, Dirent, Inode,
};
use crate::rust_kalloc::{kalloc, kfree};
use crate::rust_log::{begin_op, end_op};
use crate::rust_pipe::pipealloc;
use crate::rust_printf::panic;
use crate::rust_proc::myproc;
use crate::rust_string::memset;
use crate::rust_syscall::{argaddr, argint, argstr, fetchaddr, fetchstr};
use crate::rust_vm::copyout;

const NOFILE: usize = 16;
const NDEV: c_int = 10;
const MAXARG: usize = 32;
const MAXPATH: usize = 128;
const PGSIZE: c_int = 4096;
const DIRSIZ: usize = 14;

const O_RDONLY: c_int = 0x000;
const O_WRONLY: c_int = 0x001;
const O_RDWR: c_int = 0x002;
const O_CREATE: c_int = 0x200;
const O_TRUNC: c_int = 0x400;

const T_DIR: i16 = 1;
const T_FILE: i16 = 2;
const T_DEVICE: i16 = 3;

const FD_INODE: c_int = 2;
const FD_DEVICE: c_int = 3;

unsafe fn argfd(n: c_int) -> Option<(c_int, *mut File)> {
    let mut fd: c_int = 0;
    argint(n, ptr::addr_of_mut!(fd));
    if !(0..NOFILE as c_int).contains(&fd) {
        return None;
    }

    let p = myproc();
    let ofile = ptr::addr_of_mut!((*p).ofile).cast::<*mut c_void>();
    let f = (*ofile.add(fd as usize)).cast::<File>();
    if f.is_null() {
        return None;
    }

    Some((fd, f))
}

unsafe fn fdalloc(f: *mut File) -> c_int {
    let p = myproc();
    let ofile = ptr::addr_of_mut!((*p).ofile).cast::<*mut c_void>();
    let mut fd = 0usize;
    while fd < NOFILE {
        if (*ofile.add(fd)).is_null() {
            *ofile.add(fd) = f.cast::<c_void>();
            return fd as c_int;
        }
        fd += 1;
    }
    -1
}

unsafe fn isdirempty(dp: *mut Inode) -> c_int {
    let mut off: c_uint = (2 * mem::size_of::<Dirent>()) as c_uint;
    let mut de = Dirent {
        inum: 0,
        name: [0; DIRSIZ],
    };

    while off < (*dp).size {
        if readi(
            dp,
            0,
            ptr::addr_of_mut!(de) as u64,
            off,
            mem::size_of::<Dirent>() as c_uint,
        ) != mem::size_of::<Dirent>() as c_int
        {
            panic(b"isdirempty: readi\0".as_ptr().cast_mut().cast());
        }
        if de.inum != 0 {
            return 0;
        }
        off = off.wrapping_add(mem::size_of::<Dirent>() as c_uint);
    }

    1
}

unsafe fn create(path: *mut c_char, file_type: i16, major: i16, minor: i16) -> *mut Inode {
    let mut name = [0 as c_char; DIRSIZ];
    let dp = nameiparent(path, name.as_mut_ptr());
    if dp.is_null() {
        return ptr::null_mut();
    }

    ilock(dp);

    let mut ip = dirlookup(dp, name.as_mut_ptr(), ptr::null_mut());
    if !ip.is_null() {
        iunlockput(dp);
        ilock(ip);
        if file_type == T_FILE && ((*ip).inode_type == T_FILE || (*ip).inode_type == T_DEVICE) {
            return ip;
        }
        iunlockput(ip);
        return ptr::null_mut();
    }

    ip = ialloc((*dp).dev, file_type);
    if ip.is_null() {
        iunlockput(dp);
        return ptr::null_mut();
    }

    ilock(ip);
    (*ip).major = major;
    (*ip).minor = minor;
    (*ip).nlink = 1;
    iupdate(ip);

    if file_type == T_DIR {
        if dirlink(ip, b".\0".as_ptr().cast_mut().cast(), (*ip).inum) < 0
            || dirlink(ip, b"..\0".as_ptr().cast_mut().cast(), (*dp).inum) < 0
        {
            goto_fail(ip, dp);
            return ptr::null_mut();
        }
    }

    if dirlink(dp, name.as_mut_ptr(), (*ip).inum) < 0 {
        goto_fail(ip, dp);
        return ptr::null_mut();
    }

    if file_type == T_DIR {
        (*dp).nlink += 1;
        iupdate(dp);
    }

    iunlockput(dp);
    ip
}

unsafe fn goto_fail(ip: *mut Inode, dp: *mut Inode) {
    (*ip).nlink = 0;
    iupdate(ip);
    iunlockput(ip);
    iunlockput(dp);
}

#[no_mangle]
pub unsafe extern "C" fn sys_dup() -> u64 {
    let Some((_fd, f)) = argfd(0) else {
        return u64::MAX;
    };

    let fd = fdalloc(f);
    if fd < 0 {
        return u64::MAX;
    }

    filedup(f);
    fd as u64
}

#[no_mangle]
pub unsafe extern "C" fn sys_read() -> u64 {
    let mut p: u64 = 0;
    let mut n: c_int = 0;
    argaddr(1, ptr::addr_of_mut!(p));
    argint(2, ptr::addr_of_mut!(n));

    let Some((_fd, f)) = argfd(0) else {
        return u64::MAX;
    };

    fileread(f, p, n) as u64
}

#[no_mangle]
pub unsafe extern "C" fn sys_write() -> u64 {
    let mut p: u64 = 0;
    let mut n: c_int = 0;
    argaddr(1, ptr::addr_of_mut!(p));
    argint(2, ptr::addr_of_mut!(n));

    let Some((_fd, f)) = argfd(0) else {
        return u64::MAX;
    };

    filewrite(f, p, n) as u64
}

#[no_mangle]
pub unsafe extern "C" fn sys_close() -> u64 {
    let Some((fd, f)) = argfd(0) else {
        return u64::MAX;
    };

    let p = myproc();
    *ptr::addr_of_mut!((*p).ofile).cast::<*mut c_void>().add(fd as usize) = ptr::null_mut();
    fileclose(f);
    0
}

#[no_mangle]
pub unsafe extern "C" fn sys_fstat() -> u64 {
    let mut st: u64 = 0;
    argaddr(1, ptr::addr_of_mut!(st));

    let Some((_fd, f)) = argfd(0) else {
        return u64::MAX;
    };

    filestat(f, st) as u64
}

#[no_mangle]
pub unsafe extern "C" fn sys_link() -> u64 {
    let mut name = [0 as c_char; DIRSIZ];
    let mut new = [0 as c_char; MAXPATH];
    let mut old = [0 as c_char; MAXPATH];

    if argstr(0, old.as_mut_ptr(), MAXPATH as c_int) < 0
        || argstr(1, new.as_mut_ptr(), MAXPATH as c_int) < 0
    {
        return u64::MAX;
    }

    begin_op();
    let ip = namei(old.as_mut_ptr());
    if ip.is_null() {
        end_op();
        return u64::MAX;
    }

    ilock(ip);
    if (*ip).inode_type == T_DIR {
        iunlockput(ip);
        end_op();
        return u64::MAX;
    }

    (*ip).nlink += 1;
    iupdate(ip);
    iunlock(ip);

    let dp = nameiparent(new.as_mut_ptr(), name.as_mut_ptr());
    if dp.is_null() {
        ilock(ip);
        (*ip).nlink -= 1;
        iupdate(ip);
        iunlockput(ip);
        end_op();
        return u64::MAX;
    }

    ilock(dp);
    if (*dp).dev != (*ip).dev || dirlink(dp, name.as_mut_ptr(), (*ip).inum) < 0 {
        iunlockput(dp);
        ilock(ip);
        (*ip).nlink -= 1;
        iupdate(ip);
        iunlockput(ip);
        end_op();
        return u64::MAX;
    }

    iunlockput(dp);
    iput(ip);
    end_op();
    0
}

#[no_mangle]
pub unsafe extern "C" fn sys_unlink() -> u64 {
    let mut de = Dirent {
        inum: 0,
        name: [0; DIRSIZ],
    };
    let mut name = [0 as c_char; DIRSIZ];
    let mut path = [0 as c_char; MAXPATH];
    let mut off: c_uint = 0;

    if argstr(0, path.as_mut_ptr(), MAXPATH as c_int) < 0 {
        return u64::MAX;
    }

    begin_op();
    let dp = nameiparent(path.as_mut_ptr(), name.as_mut_ptr());
    if dp.is_null() {
        end_op();
        return u64::MAX;
    }

    ilock(dp);

    if namecmp(name.as_ptr(), b".\0".as_ptr().cast()) == 0
        || namecmp(name.as_ptr(), b"..\0".as_ptr().cast()) == 0
    {
        iunlockput(dp);
        end_op();
        return u64::MAX;
    }

    let ip = dirlookup(dp, name.as_mut_ptr(), ptr::addr_of_mut!(off));
    if ip.is_null() {
        iunlockput(dp);
        end_op();
        return u64::MAX;
    }
    ilock(ip);

    if (*ip).nlink < 1 {
        panic(b"unlink: nlink < 1\0".as_ptr().cast_mut().cast());
    }

    if (*ip).inode_type == T_DIR && isdirempty(ip) == 0 {
        iunlockput(ip);
        iunlockput(dp);
        end_op();
        return u64::MAX;
    }

    memset(
        ptr::addr_of_mut!(de).cast::<c_void>(),
        0,
        mem::size_of::<Dirent>() as c_uint,
    );
    if writei(
        dp,
        0,
        ptr::addr_of_mut!(de) as u64,
        off,
        mem::size_of::<Dirent>() as c_uint,
    ) != mem::size_of::<Dirent>() as c_int
    {
        panic(b"unlink: writei\0".as_ptr().cast_mut().cast());
    }

    if (*ip).inode_type == T_DIR {
        (*dp).nlink -= 1;
        iupdate(dp);
    }
    iunlockput(dp);

    (*ip).nlink -= 1;
    iupdate(ip);
    iunlockput(ip);

    end_op();
    0
}

#[no_mangle]
pub unsafe extern "C" fn sys_open() -> u64 {
    let mut path = [0 as c_char; MAXPATH];
    let mut omode: c_int = 0;

    argint(1, ptr::addr_of_mut!(omode));
    if argstr(0, path.as_mut_ptr(), MAXPATH as c_int) < 0 {
        return u64::MAX;
    }

    begin_op();

    let ip = if (omode & O_CREATE) != 0 {
        let created = create(path.as_mut_ptr(), T_FILE, 0, 0);
        if created.is_null() {
            end_op();
            return u64::MAX;
        }
        created
    } else {
        let found = namei(path.as_mut_ptr());
        if found.is_null() {
            end_op();
            return u64::MAX;
        }

        ilock(found);
        if (*found).inode_type == T_DIR && omode != O_RDONLY {
            iunlockput(found);
            end_op();
            return u64::MAX;
        }
        found
    };

    if (*ip).inode_type == T_DEVICE && ((*ip).major < 0 || (*ip).major >= NDEV as i16) {
        iunlockput(ip);
        end_op();
        return u64::MAX;
    }

    let f = filealloc();
    let fd = if !f.is_null() { fdalloc(f) } else { -1 };
    if f.is_null() || fd < 0 {
        if !f.is_null() {
            fileclose(f);
        }
        iunlockput(ip);
        end_op();
        return u64::MAX;
    }

    if (*ip).inode_type == T_DEVICE {
        (*f).file_type = FD_DEVICE;
        (*f).major = (*ip).major as c_short;
    } else {
        (*f).file_type = FD_INODE;
        (*f).off = 0;
    }

    (*f).ip = ip.cast::<c_void>();
    (*f).readable = if (omode & O_WRONLY) != 0 { 0 } else { 1 };
    (*f).writable = if (omode & O_WRONLY) != 0 || (omode & O_RDWR) != 0 {
        1
    } else {
        0
    };

    if (omode & O_TRUNC) != 0 && (*ip).inode_type == T_FILE {
        itrunc(ip);
    }

    iunlock(ip);
    end_op();

    fd as u64
}

#[no_mangle]
pub unsafe extern "C" fn sys_mkdir() -> u64 {
    let mut path = [0 as c_char; MAXPATH];

    begin_op();
    let ip = if argstr(0, path.as_mut_ptr(), MAXPATH as c_int) < 0 {
        ptr::null_mut()
    } else {
        create(path.as_mut_ptr(), T_DIR, 0, 0)
    };

    if ip.is_null() {
        end_op();
        return u64::MAX;
    }

    iunlockput(ip);
    end_op();
    0
}

#[no_mangle]
pub unsafe extern "C" fn sys_mknod() -> u64 {
    let mut path = [0 as c_char; MAXPATH];
    let mut major: c_int = 0;
    let mut minor: c_int = 0;

    begin_op();
    argint(1, ptr::addr_of_mut!(major));
    argint(2, ptr::addr_of_mut!(minor));

    let ip = if argstr(0, path.as_mut_ptr(), MAXPATH as c_int) < 0 {
        ptr::null_mut()
    } else {
        create(path.as_mut_ptr(), T_DEVICE, major as i16, minor as i16)
    };

    if ip.is_null() {
        end_op();
        return u64::MAX;
    }

    iunlockput(ip);
    end_op();
    0
}

#[no_mangle]
pub unsafe extern "C" fn sys_chdir() -> u64 {
    let mut path = [0 as c_char; MAXPATH];
    let p = myproc();

    begin_op();
    let ip = if argstr(0, path.as_mut_ptr(), MAXPATH as c_int) < 0 {
        ptr::null_mut()
    } else {
        namei(path.as_mut_ptr())
    };

    if ip.is_null() {
        end_op();
        return u64::MAX;
    }

    ilock(ip);
    if (*ip).inode_type != T_DIR {
        iunlockput(ip);
        end_op();
        return u64::MAX;
    }

    iunlock(ip);
    iput((*p).cwd.cast::<Inode>());
    end_op();
    (*p).cwd = ip.cast::<c_void>();

    0
}

#[no_mangle]
pub unsafe extern "C" fn sys_exec() -> u64 {
    let mut path = [0 as c_char; MAXPATH];
    let mut argv = [ptr::null_mut::<c_char>(); MAXARG];
    let mut uargv: u64 = 0;

    argaddr(1, ptr::addr_of_mut!(uargv));
    if argstr(0, path.as_mut_ptr(), MAXPATH as c_int) < 0 {
        return u64::MAX;
    }

    let mut i = 0usize;
    while i < MAXARG {
        let mut uarg: u64 = 0;
        if fetchaddr(
            uargv.wrapping_add((mem::size_of::<u64>() * i) as u64),
            ptr::addr_of_mut!(uarg),
        ) < 0
        {
            break_bad(&mut argv);
            return u64::MAX;
        }

        if uarg == 0 {
            argv[i] = ptr::null_mut();
            let ret = kexec(path.as_mut_ptr(), argv.as_mut_ptr());
            free_argv(&mut argv);
            return ret as u64;
        }

        argv[i] = kalloc().cast::<c_char>();
        if argv[i].is_null() {
            break_bad(&mut argv);
            return u64::MAX;
        }

        if fetchstr(uarg, argv[i], PGSIZE) < 0 {
            break_bad(&mut argv);
            return u64::MAX;
        }

        i += 1;
    }

    break_bad(&mut argv);
    u64::MAX
}

unsafe fn free_argv(argv: &mut [*mut c_char; MAXARG]) {
    let mut i = 0usize;
    while i < MAXARG {
        if argv[i].is_null() {
            break;
        }
        kfree(argv[i].cast::<c_void>());
        i += 1;
    }
}

unsafe fn break_bad(argv: &mut [*mut c_char; MAXARG]) {
    free_argv(argv);
}

#[no_mangle]
pub unsafe extern "C" fn sys_pipe() -> u64 {
    let mut fdarray: u64 = 0;
    let mut rf: *mut File = ptr::null_mut();
    let mut wf: *mut File = ptr::null_mut();

    argaddr(0, ptr::addr_of_mut!(fdarray));
    if pipealloc(ptr::addr_of_mut!(rf), ptr::addr_of_mut!(wf)) < 0 {
        return u64::MAX;
    }

    let p = myproc();
    let fd0 = fdalloc(rf);
    if fd0 < 0 {
        fileclose(rf);
        fileclose(wf);
        return u64::MAX;
    }

    let fd1 = fdalloc(wf);
    if fd1 < 0 {
        *ptr::addr_of_mut!((*p).ofile).cast::<*mut c_void>().add(fd0 as usize) = ptr::null_mut();
        fileclose(rf);
        fileclose(wf);
        return u64::MAX;
    }


    if copyout(
        (*p).pagetable,
        fdarray,
        ptr::addr_of!(fd0).cast::<c_char>().cast_mut(),
        mem::size_of::<c_int>() as u64,
    ) < 0
        || copyout(
            (*p).pagetable,
            fdarray.wrapping_add(mem::size_of::<c_int>() as u64),
            ptr::addr_of!(fd1).cast::<c_char>().cast_mut(),
            mem::size_of::<c_int>() as u64,
    ) < 0
    {
        let ofile = ptr::addr_of_mut!((*p).ofile).cast::<*mut c_void>();
        *ofile.add(fd0 as usize) = ptr::null_mut();
        *ofile.add(fd1 as usize) = ptr::null_mut();
        fileclose(rf);
        fileclose(wf);
        return u64::MAX;
    }

    0
}
