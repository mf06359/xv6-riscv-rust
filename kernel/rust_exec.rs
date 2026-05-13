use core::ffi::{c_char, c_int, c_uint};
use core::mem;
use core::ptr;

use crate::rust_fs::{ilock, iunlockput, namei, readi, Inode};
use crate::rust_log::{begin_op, end_op};
use crate::rust_printf::panic;
use crate::rust_proc::{myproc, proc_freepagetable, proc_pagetable};
use crate::rust_string::{safestrcpy, strlen};
use crate::rust_vm::{copyout, uvmalloc, uvmclear, walkaddr};

const MAXARG: usize = 32;
const USERSTACK: u64 = 1;
const PGSIZE: u64 = 4096;
const PTE_W: c_int = 1 << 2;
const PTE_X: c_int = 1 << 3;
const ELF_MAGIC: c_uint = 0x464c457f;
const ELF_PROG_LOAD: c_uint = 1;

#[repr(C)]
struct Elfhdr {
    magic: c_uint,
    elf: [u8; 12],
    elf_type: u16,
    machine: u16,
    version: c_uint,
    entry: u64,
    phoff: u64,
    shoff: u64,
    flags: c_uint,
    ehsize: u16,
    phentsize: u16,
    phnum: u16,
    shentsize: u16,
    shnum: u16,
    shstrndx: u16,
}

#[repr(C)]
struct Proghdr {
    prog_type: c_uint,
    flags: c_uint,
    off: u64,
    vaddr: u64,
    paddr: u64,
    filesz: u64,
    memsz: u64,
    align: u64,
}

#[inline(always)]
fn pgroundup(sz: u64) -> u64 {
    (sz + PGSIZE - 1) & !(PGSIZE - 1)
}

#[inline(always)]
fn flags2perm(flags: c_uint) -> c_int {
    let mut perm = 0;
    if (flags & 0x1) != 0 {
        perm = PTE_X;
    }
    if (flags & 0x2) != 0 {
        perm |= PTE_W;
    }
    perm
}

unsafe fn loadseg(pagetable: *mut u64, va: u64, ip: *mut Inode, offset: c_uint, sz: c_uint) -> c_int {
    let mut i: c_uint = 0;
    while i < sz {
        let pa = walkaddr(pagetable, va.wrapping_add(i as u64));
        if pa == 0 {
            panic(b"loadseg: address should exist\0".as_ptr().cast_mut().cast());
        }

        let n = if sz.wrapping_sub(i) < PGSIZE as c_uint {
            sz.wrapping_sub(i)
        } else {
            PGSIZE as c_uint
        };

        if readi(ip, 0, pa, offset.wrapping_add(i), n) != n as c_int {
            return -1;
        }
        i = i.wrapping_add(PGSIZE as c_uint);
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn kexec(path: *mut c_char, argv: *mut *mut c_char) -> c_int {
    let mut argc: u64 = 0;
    let mut sz: u64 = 0;
    let mut sp: u64;
    let stackbase: u64;
    let mut ustack = [0u64; MAXARG + 1];
    let mut elf: Elfhdr = mem::zeroed();
    let mut ph: Proghdr = mem::zeroed();
    let mut ip: *mut Inode;
    let mut pagetable: *mut u64 = ptr::null_mut();
    let oldpagetable: *mut u64;
    let mut p = myproc();
    let oldsz: u64;
    let mut ok = false;

    begin_op();

    loop {
        ip = namei(path);
        if ip.is_null() {
            end_op();
            break;
        }
        ilock(ip);

        if readi(
            ip,
            0,
            ptr::addr_of_mut!(elf) as u64,
            0,
            mem::size_of::<Elfhdr>() as c_uint,
        ) != mem::size_of::<Elfhdr>() as c_int
        {
            break;
        }

        if elf.magic != ELF_MAGIC {
            break;
        }

        pagetable = proc_pagetable(p);
        if pagetable.is_null() {
            break;
        }

        let mut i: c_int = 0;
        let mut off: c_int = elf.phoff as c_int;
        while i < elf.phnum as c_int {
            if readi(
                ip,
                0,
                ptr::addr_of_mut!(ph) as u64,
                off as c_uint,
                mem::size_of::<Proghdr>() as c_uint,
            ) != mem::size_of::<Proghdr>() as c_int
            {
                break;
            }
            off = off.wrapping_add(mem::size_of::<Proghdr>() as c_int);

            if ph.prog_type != ELF_PROG_LOAD {
                i += 1;
                continue;
            }
            if ph.memsz < ph.filesz {
                break;
            }
            let end_va = ph.vaddr.wrapping_add(ph.memsz);
            if end_va < ph.vaddr {
                break;
            }
            if (ph.vaddr % PGSIZE) != 0 {
                break;
            }

            let sz1 = uvmalloc(pagetable, sz, end_va, flags2perm(ph.flags));
            if sz1 == 0 {
                break;
            }
            sz = sz1;

            if loadseg(pagetable, ph.vaddr, ip, ph.off as c_uint, ph.filesz as c_uint) < 0 {
                break;
            }
            i += 1;
        }

        if i < elf.phnum as c_int {
            break;
        }

        iunlockput(ip);
        end_op();
        ip = ptr::null_mut();

        p = myproc();
        oldsz = (*p).sz;

        sz = pgroundup(sz);
        let sz1 = uvmalloc(pagetable, sz, sz + (USERSTACK + 1) * PGSIZE, PTE_W);
        if sz1 == 0 {
            break;
        }
        sz = sz1;

        uvmclear(pagetable, sz - (USERSTACK + 1) * PGSIZE);
        sp = sz;
        stackbase = sp - USERSTACK * PGSIZE;

        argc = 0;
        while !(*argv.add(argc as usize)).is_null() {
            if argc >= MAXARG as u64 {
                break;
            }

            let arg = *argv.add(argc as usize);
            let arglen = strlen(arg) as u64 + 1;
            sp = sp.wrapping_sub(arglen);
            sp = sp.wrapping_sub(sp % 16);
            if sp < stackbase {
                break;
            }
            if copyout(pagetable, sp, arg, arglen) < 0 {
                break;
            }
            *ustack.as_mut_ptr().add(argc as usize) = sp;
            argc += 1;
        }
        if (*argv.add(argc as usize)).is_null() == false {
            break;
        }
        *ustack.as_mut_ptr().add(argc as usize) = 0;

        sp = sp.wrapping_sub((argc + 1) * mem::size_of::<u64>() as u64);
        sp = sp.wrapping_sub(sp % 16);
        if sp < stackbase {
            break;
        }
        if copyout(
            pagetable,
            sp,
            ustack.as_mut_ptr().cast::<c_char>(),
            (argc + 1) * mem::size_of::<u64>() as u64,
        ) < 0
        {
            break;
        }

        (*(*p).trapframe).a1 = sp;

        let mut s = path;
        let mut last = path;
        while *s != 0 {
            if *s == b'/' as c_char {
                last = s.add(1);
            }
            s = s.add(1);
        }
        safestrcpy((*p).name.as_mut_ptr().cast::<c_char>(), last, (*p).name.len() as c_int);

        oldpagetable = (*p).pagetable;
        (*p).pagetable = pagetable;
        (*p).sz = sz;
        (*(*p).trapframe).epc = elf.entry;
        (*(*p).trapframe).sp = sp;
        proc_freepagetable(oldpagetable, oldsz);

        ok = true;
        break;
    }

    if ok {
        argc as c_int
    } else {
        if !pagetable.is_null() {
            proc_freepagetable(pagetable, sz);
        }
        if !ip.is_null() {
            iunlockput(ip);
            end_op();
        }
        -1
    }
}
