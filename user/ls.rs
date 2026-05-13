#![no_std]

use core::ffi::{c_char, c_int, c_uint, c_void};

mod rust_user;
use rust_user::*;

static mut FMTBUF: [c_char; DIRSIZ + 1] = [0; DIRSIZ + 1];

unsafe fn fmtname(path: *mut c_char) -> *mut c_char {
    let mut p = path.add(strlen(path) as usize);
    while (p as usize) >= (path as usize) && *p != b'/' as c_char {
        if p == path {
            break;
        }
        p = p.sub(1);
    }
    if *p == b'/' as c_char {
        p = p.add(1);
    }

    let name_len = strlen(p) as usize;
    if name_len >= DIRSIZ {
        return p;
    }

    memmove(
        core::ptr::addr_of_mut!(FMTBUF).cast::<c_void>(),
        p.cast::<c_void>(),
        name_len as c_int,
    );
    memset(
        core::ptr::addr_of_mut!(FMTBUF).cast::<u8>().add(name_len).cast::<c_void>(),
        b' ' as c_int,
        (DIRSIZ - name_len) as c_uint,
    );
    *core::ptr::addr_of_mut!(FMTBUF)
        .cast::<c_char>()
        .add(DIRSIZ) = 0;
    core::ptr::addr_of_mut!(FMTBUF).cast::<c_char>()
}

unsafe fn ls(path: *mut c_char) {
    let mut buf: [c_char; 512] = [0; 512];
    let mut de = Dirent {
        inum: 0,
        name: [0; DIRSIZ],
    };
    let mut st = Stat {
        dev: 0,
        ino: 0,
        file_type: 0,
        nlink: 0,
        size: 0,
    };

    let fd = open(path, O_RDONLY);
    if fd < 0 {
        fprintf(2, b"ls: cannot open %s\n\0".as_ptr().cast(), path);
        return;
    }

    if fstat(fd, core::ptr::addr_of_mut!(st)) < 0 {
        fprintf(2, b"ls: cannot stat %s\n\0".as_ptr().cast(), path);
        close(fd);
        return;
    }

    if st.file_type == T_DEVICE || st.file_type == T_FILE {
        printf(
            b"%s %d %d %d\n\0".as_ptr().cast(),
            fmtname(path),
            st.file_type as c_int,
            st.ino as c_int,
            st.size as c_int,
        );
        close(fd);
        return;
    }

    if st.file_type == T_DIR {
        if (strlen(path) as usize) + 1 + DIRSIZ + 1 > 512 {
            printf(b"ls: path too long\n\0".as_ptr().cast());
            close(fd);
            return;
        }

        strcpy(buf.as_mut_ptr(), path);
        let mut p = buf.as_mut_ptr().add(strlen(buf.as_ptr()) as usize);
        *p = b'/' as c_char;
        p = p.add(1);

        let de_sz = core::mem::size_of::<Dirent>() as c_int;
        while read(fd, core::ptr::addr_of_mut!(de).cast::<c_void>(), de_sz) == de_sz {
            if de.inum == 0 {
                continue;
            }
            memmove(
                p.cast::<c_void>(),
                de.name.as_ptr().cast::<c_void>(),
                DIRSIZ as c_int,
            );
            *p.add(DIRSIZ) = 0;

            if stat(buf.as_ptr(), core::ptr::addr_of_mut!(st)) < 0 {
                printf(b"ls: cannot stat %s\n\0".as_ptr().cast(), buf.as_ptr());
                continue;
            }
            printf(
                b"%s %d %d %d\n\0".as_ptr().cast(),
                fmtname(buf.as_mut_ptr()),
                st.file_type as c_int,
                st.ino as c_int,
                st.size as c_int,
            );
        }
    }

    close(fd);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn main(argc: c_int, argv: *mut *mut c_char) -> c_int {
    if argc < 2 {
        ls(b".\0".as_ptr() as *mut c_char);
        return 0;
    }

    let mut i = 1;
    while i < argc {
        ls(argv_at(argv, i));
        i += 1;
    }
    0
}
