use core::ffi::{c_char, c_int, c_uint, c_void};
use core::marker::PhantomData;
use core::mem;
use core::ops::{Deref, DerefMut};
use core::ptr;

use crate::rust_bio::{bread, brelse, Buf};
use crate::rust_log::{initlog, log_write, TxnGuard};
use crate::rust_printf::{kprintf0, kprintf1, panic};
use crate::rust_proc::{either_copyin, either_copyout, myproc_cwd};
use crate::rust_sleeplock::{acquiresleep, holdingsleep, initsleeplock, releasesleep, Sleeplock};
use crate::rust_spinlock::{acquire, initlock, release, Spinlock};
use crate::rust_string::{memset, memmove, strncmp, strncpy};

const BSIZE: usize = 1024;
const DIRSIZ: usize = 14;
const NDIRECT: usize = 12;
const NINDIRECT: usize = BSIZE / core::mem::size_of::<c_uint>();
const MAXFILE: usize = NDIRECT + NINDIRECT;
const NINODE: usize = 50;
const ROOTDEV: c_uint = 1;
const ROOTINO: c_uint = 1;
const FSMAGIC: c_uint = 0x1020_3040;
const BPB: c_uint = (BSIZE * 8) as c_uint;
const T_DIR: c_int = 1;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Superblock {
    pub magic: c_uint,
    pub size: c_uint,
    pub nblocks: c_uint,
    pub ninodes: c_uint,
    pub nlog: c_uint,
    pub logstart: c_uint,
    pub inodestart: c_uint,
    pub bmapstart: c_uint,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Inode {
    pub dev: c_uint,
    pub inum: c_uint,
    pub refcnt: c_int,
    pub lock: Sleeplock,
    pub valid: c_int,
    pub inode_type: i16,
    pub major: i16,
    pub minor: i16,
    pub nlink: i16,
    pub size: c_uint,
    pub addrs: [c_uint; NDIRECT + 1],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Dinode {
    inode_type: i16,
    major: i16,
    minor: i16,
    nlink: i16,
    size: c_uint,
    addrs: [c_uint; NDIRECT + 1],
}

pub struct Itable {
    lock: Spinlock,
    inode: [Inode; NINODE],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Dirent {
    pub inum: u16,
    pub name: [c_char; DIRSIZ],
}

#[repr(C)]
pub struct Stat {
    pub dev: c_int,
    pub ino: c_uint,
    pub inode_type: i16,
    pub nlink: i16,
    pub size: u64,
}

const EMPTY_SPINLOCK: Spinlock = Spinlock {
    locked: 0,
    name: ptr::null_mut(),
    cpu: ptr::null_mut(),
};

const EMPTY_SLEEPLOCK: Sleeplock = Sleeplock {
    locked: 0,
    lk: EMPTY_SPINLOCK,
    name: ptr::null_mut(),
    pid: 0,
};

const EMPTY_INODE: Inode = Inode {
    dev: 0,
    inum: 0,
    refcnt: 0,
    lock: EMPTY_SLEEPLOCK,
    valid: 0,
    inode_type: 0,
    major: 0,
    minor: 0,
    nlink: 0,
    size: 0,
    addrs: [0; NDIRECT + 1],
};

#[no_mangle]
pub static mut sb: Superblock = Superblock {
    magic: 0,
    size: 0,
    nblocks: 0,
    ninodes: 0,
    nlog: 0,
    logstart: 0,
    inodestart: 0,
    bmapstart: 0,
};

#[no_mangle]
pub static mut itable: Itable = Itable {
    lock: EMPTY_SPINLOCK,
    inode: [EMPTY_INODE; NINODE],
};


const IPB: c_uint = (BSIZE / core::mem::size_of::<Dinode>()) as c_uint;

#[inline(always)]
unsafe fn iblock(inum: c_uint) -> c_uint {
    inum / IPB + sb.inodestart
}

#[inline(always)]
unsafe fn bblock(b: c_uint) -> c_uint {
    b / BPB + sb.bmapstart
}

#[inline(always)]
unsafe fn dinode_ptr(bp: *mut Buf, inum: c_uint) -> *mut Dinode {
    ptr::addr_of_mut!((*bp).data)
        .cast::<Dinode>()
        .add((inum % IPB) as usize)
}

#[inline(always)]
fn min_u(a: c_uint, b: c_uint) -> c_uint {
    if a < b {
        a
    } else {
        b
    }
}

unsafe fn readsb(dev: c_int, sb_out: *mut Superblock) {
    let bp = bread(dev as c_uint, 1);
    memmove(
        sb_out.cast::<c_void>(),
        ptr::addr_of!((*bp).data).cast::<c_void>(),
        core::mem::size_of::<Superblock>() as c_uint,
    );
    brelse(bp);
}

unsafe fn bzero(dev: c_int, bno: c_int) {
    let bp = bread(dev as c_uint, bno as c_uint);
    memset(ptr::addr_of_mut!((*bp).data).cast::<c_void>(), 0, BSIZE as c_uint);
    log_write(bp);
    brelse(bp);
}

#[no_mangle]
pub unsafe extern "C" fn fsinit(dev: c_int) {
    readsb(dev, ptr::addr_of_mut!(sb));
    if sb.magic != FSMAGIC {
        panic(b"invalid file system\0".as_ptr().cast_mut().cast());
    }
    initlog(dev, ptr::addr_of_mut!(sb));
    ireclaim(dev);
}

#[no_mangle]
pub unsafe extern "C" fn iinit() {
    initlock(
        ptr::addr_of_mut!(itable.lock),
        b"itable\0".as_ptr().cast_mut().cast(),
    );

    let mut i = 0usize;
    while i < NINODE {
        initsleeplock(
            ptr::addr_of_mut!(itable.inode[i].lock),
            b"inode\0".as_ptr().cast_mut().cast(),
        );
        i += 1;
    }
}

unsafe fn balloc(dev: c_uint) -> c_uint {
    let mut b: c_uint = 0;
    while b < sb.size {
        let bp = bread(dev, bblock(b));

        let mut bi: c_uint = 0;
        while bi < BPB && b + bi < sb.size {
            let m = 1 << (bi % 8);
            if ((*bp).data[(bi / 8) as usize] as c_int & m) == 0 {
                (*bp).data[(bi / 8) as usize] |= m as u8;
                log_write(bp);
                brelse(bp);
                bzero(dev as c_int, (b + bi) as c_int);
                return b + bi;
            }
            bi += 1;
        }

        brelse(bp);
        b = b.wrapping_add(BPB);
    }

    kprintf0(b"balloc: out of blocks\n\0".as_ptr().cast());
    0
}

unsafe fn bfree(dev: c_int, b: c_uint) {
    let bp = bread(dev as c_uint, bblock(b));
    let bi = b % BPB;
    let m = 1 << (bi % 8);

    if ((*bp).data[(bi / 8) as usize] as c_int & m) == 0 {
        panic(b"freeing free block\0".as_ptr().cast_mut().cast());
    }

    (*bp).data[(bi / 8) as usize] &= !(m as u8);
    log_write(bp);
    brelse(bp);
}

unsafe fn iget(dev: c_uint, inum: c_uint) -> *mut Inode {
    acquire(ptr::addr_of_mut!(itable.lock));

    let mut empty: *mut Inode = ptr::null_mut();
    let mut i = 0usize;
    while i < NINODE {
        let ip = ptr::addr_of_mut!(itable.inode[i]);
        if (*ip).refcnt > 0 && (*ip).dev == dev && (*ip).inum == inum {
            (*ip).refcnt += 1;
            release(ptr::addr_of_mut!(itable.lock));
            return ip;
        }
        if empty.is_null() && (*ip).refcnt == 0 {
            empty = ip;
        }
        i += 1;
    }

    if empty.is_null() {
        panic(b"iget: no inodes\0".as_ptr().cast_mut().cast());
    }

    (*empty).dev = dev;
    (*empty).inum = inum;
    (*empty).refcnt = 1;
    (*empty).valid = 0;

    release(ptr::addr_of_mut!(itable.lock));
    empty
}

#[no_mangle]
pub unsafe extern "C" fn ialloc(dev: c_uint, inode_type: i16) -> *mut Inode {
    let mut inum: c_uint = 1;
    while inum < sb.ninodes {
        let bp = bread(dev, iblock(inum));
        let dip = dinode_ptr(bp, inum);
        if (*dip).inode_type == 0 {
            memset(
                dip.cast::<c_void>(),
                0,
                core::mem::size_of::<Dinode>() as c_uint,
            );
            (*dip).inode_type = inode_type;
            log_write(bp);
            brelse(bp);
            return iget(dev, inum);
        }
        brelse(bp);
        inum = inum.wrapping_add(1);
    }
    kprintf0(b"ialloc: no inodes\n\0".as_ptr().cast());
    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn iupdate(ip: *mut Inode) {
    let bp = bread((*ip).dev, iblock((*ip).inum));
    let dip = dinode_ptr(bp, (*ip).inum);
    (*dip).inode_type = (*ip).inode_type;
    (*dip).major = (*ip).major;
    (*dip).minor = (*ip).minor;
    (*dip).nlink = (*ip).nlink;
    (*dip).size = (*ip).size;
    memmove(
        ptr::addr_of_mut!((*dip).addrs).cast::<c_void>(),
        ptr::addr_of!((*ip).addrs).cast::<c_void>(),
        core::mem::size_of_val(&(*ip).addrs) as c_uint,
    );
    log_write(bp);
    brelse(bp);
}

#[no_mangle]
pub unsafe extern "C" fn idup(ip: *mut Inode) -> *mut Inode {
    acquire(ptr::addr_of_mut!(itable.lock));
    (*ip).refcnt += 1;
    release(ptr::addr_of_mut!(itable.lock));
    ip
}

#[no_mangle]
pub unsafe extern "C" fn ilock(ip: *mut Inode) {
    if ip.is_null() || (*ip).refcnt < 1 {
        panic(b"ilock\0".as_ptr().cast_mut().cast());
    }

    acquiresleep(ptr::addr_of_mut!((*ip).lock));

    if (*ip).valid == 0 {
        let bp = bread((*ip).dev, iblock((*ip).inum));
        let dip = dinode_ptr(bp, (*ip).inum);
        (*ip).inode_type = (*dip).inode_type;
        (*ip).major = (*dip).major;
        (*ip).minor = (*dip).minor;
        (*ip).nlink = (*dip).nlink;
        (*ip).size = (*dip).size;
        memmove(
            ptr::addr_of_mut!((*ip).addrs).cast::<c_void>(),
            ptr::addr_of!((*dip).addrs).cast::<c_void>(),
            core::mem::size_of_val(&(*ip).addrs) as c_uint,
        );
        brelse(bp);
        (*ip).valid = 1;
        if (*ip).inode_type == 0 {
            panic(b"ilock: no type\0".as_ptr().cast_mut().cast());
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn iunlock(ip: *mut Inode) {
    if ip.is_null() || holdingsleep(ptr::addr_of_mut!((*ip).lock)) == 0 || (*ip).refcnt < 1 {
        panic(b"iunlock\0".as_ptr().cast_mut().cast());
    }
    releasesleep(ptr::addr_of_mut!((*ip).lock));
}

/// RAII helper for inode sleep-locking.
///
/// Construct with `InodeGuard::lock(ip)`. The inode is automatically
/// unlocked when the guard goes out of scope.
#[must_use = "inode remains locked until this guard is dropped"]
pub struct InodeGuard<'a> {
    ip: *mut Inode,
    _borrow: PhantomData<&'a mut Inode>,
}

impl<'a> InodeGuard<'a> {
    #[inline]
    pub unsafe fn lock(ip: *mut Inode) -> Self {
        ilock(ip);
        Self {
            ip,
            _borrow: PhantomData,
        }
    }

    #[inline]
    pub fn as_ptr(&self) -> *mut Inode {
        self.ip
    }

    /// Unlock and drop one reference (`iunlockput`) in one step.
    #[inline]
    pub unsafe fn unlock_put(self) {
        let ip = self.ip;
        core::mem::forget(self);
        iunlockput(ip);
    }
}

impl Deref for InodeGuard<'_> {
    type Target = Inode;
    fn deref(&self) -> &Inode {
        unsafe { &*self.ip }
    }
}

impl DerefMut for InodeGuard<'_> {
    fn deref_mut(&mut self) -> &mut Inode {
        unsafe { &mut *self.ip }
    }
}

impl Drop for InodeGuard<'_> {
    #[inline]
    fn drop(&mut self) {
        unsafe { iunlock(self.ip) };
    }
}

#[no_mangle]
pub unsafe extern "C" fn iput(ip: *mut Inode) {
    acquire(ptr::addr_of_mut!(itable.lock));

    if (*ip).refcnt == 1 && (*ip).valid != 0 && (*ip).nlink == 0 {
        acquiresleep(ptr::addr_of_mut!((*ip).lock));
        release(ptr::addr_of_mut!(itable.lock));

        itrunc(ip);
        (*ip).inode_type = 0;
        iupdate(ip);
        (*ip).valid = 0;

        releasesleep(ptr::addr_of_mut!((*ip).lock));

        acquire(ptr::addr_of_mut!(itable.lock));
    }

    (*ip).refcnt -= 1;
    release(ptr::addr_of_mut!(itable.lock));
}

#[no_mangle]
pub unsafe extern "C" fn iunlockput(ip: *mut Inode) {
    iunlock(ip);
    iput(ip);
}

#[no_mangle]
pub unsafe extern "C" fn ireclaim(dev: c_int) {
    let mut inum: c_int = 1;
    while inum < sb.ninodes as c_int {
        let mut ip: *mut Inode = ptr::null_mut();
        let bp = bread(dev as c_uint, iblock(inum as c_uint));
        let dip = dinode_ptr(bp, inum as c_uint);
        if (*dip).inode_type != 0 && (*dip).nlink == 0 {
            kprintf1(b"ireclaim: orphaned inode %d\n\0".as_ptr().cast(), inum as u64);
            ip = iget(dev as c_uint, inum as c_uint);
        }
        brelse(bp);

        if !ip.is_null() {
            let _tx = TxnGuard::begin();
            ilock(ip);
            iunlock(ip);
            iput(ip);
        }

        inum += 1;
    }
}

unsafe fn bmap(ip: *mut Inode, mut bn: c_uint) -> c_uint {
    if bn < NDIRECT as c_uint {
        if (*ip).addrs[bn as usize] == 0 {
            let addr = balloc((*ip).dev);
            if addr == 0 {
                return 0;
            }
            (*ip).addrs[bn as usize] = addr;
        }
        return (*ip).addrs[bn as usize];
    }

    bn -= NDIRECT as c_uint;

    if bn < NINDIRECT as c_uint {
        if (*ip).addrs[NDIRECT] == 0 {
            let addr = balloc((*ip).dev);
            if addr == 0 {
                return 0;
            }
            (*ip).addrs[NDIRECT] = addr;
        }

        let bp = bread((*ip).dev, (*ip).addrs[NDIRECT]);
        let a = ptr::addr_of_mut!((*bp).data).cast::<c_uint>();

        if *a.add(bn as usize) == 0 {
            let addr = balloc((*ip).dev);
            if addr != 0 {
                *a.add(bn as usize) = addr;
                log_write(bp);
            }
        }

        let out = *a.add(bn as usize);
        brelse(bp);
        return out;
    }

    panic(b"bmap: out of range\0".as_ptr().cast_mut().cast())
}

#[no_mangle]
pub unsafe extern "C" fn itrunc(ip: *mut Inode) {
    let mut i = 0usize;
    while i < NDIRECT {
        if (*ip).addrs[i] != 0 {
            bfree((*ip).dev as c_int, (*ip).addrs[i]);
            (*ip).addrs[i] = 0;
        }
        i += 1;
    }

    if (*ip).addrs[NDIRECT] != 0 {
        let bp = bread((*ip).dev, (*ip).addrs[NDIRECT]);
        let a = ptr::addr_of_mut!((*bp).data).cast::<c_uint>();
        let mut j = 0usize;
        while j < NINDIRECT {
            if *a.add(j) != 0 {
                bfree((*ip).dev as c_int, *a.add(j));
            }
            j += 1;
        }
        brelse(bp);
        bfree((*ip).dev as c_int, (*ip).addrs[NDIRECT]);
        (*ip).addrs[NDIRECT] = 0;
    }

    (*ip).size = 0;
    iupdate(ip);
}

#[no_mangle]
pub unsafe extern "C" fn stati(ip: *mut Inode, st: *mut Stat) {
    (*st).dev = (*ip).dev as c_int;
    (*st).ino = (*ip).inum;
    (*st).inode_type = (*ip).inode_type;
    (*st).nlink = (*ip).nlink;
    (*st).size = (*ip).size as u64;
}

#[no_mangle]
pub unsafe extern "C" fn readi(
    ip: *mut Inode,
    user_dst: c_int,
    mut dst: u64,
    mut off: c_uint,
    mut n: c_uint,
) -> c_int {
    if off > (*ip).size || off.wrapping_add(n) < off {
        return 0;
    }
    if off.wrapping_add(n) > (*ip).size {
        n = (*ip).size.wrapping_sub(off);
    }

    let mut tot: c_uint = 0;
    while tot < n {
        let addr = bmap(ip, off / BSIZE as c_uint);
        if addr == 0 {
            break;
        }

        let bp = bread((*ip).dev, addr);
        let m = min_u(n - tot, BSIZE as c_uint - off % BSIZE as c_uint);
        if either_copyout(
            user_dst,
            dst,
            ptr::addr_of_mut!((*bp).data[(off % BSIZE as c_uint) as usize]).cast::<c_void>(),
            m as u64,
        ) == -1
        {
            brelse(bp);
            return -1;
        }
        brelse(bp);

        tot = tot.wrapping_add(m);
        off = off.wrapping_add(m);
        dst = dst.wrapping_add(m as u64);
    }

    tot as c_int
}

#[no_mangle]
pub unsafe extern "C" fn writei(
    ip: *mut Inode,
    user_src: c_int,
    mut src: u64,
    mut off: c_uint,
    n: c_uint,
) -> c_int {
    if off > (*ip).size || off.wrapping_add(n) < off {
        return -1;
    }
    if off.wrapping_add(n) > (MAXFILE * BSIZE) as c_uint {
        return -1;
    }

    let mut tot: c_uint = 0;
    while tot < n {
        let addr = bmap(ip, off / BSIZE as c_uint);
        if addr == 0 {
            break;
        }

        let bp = bread((*ip).dev, addr);
        let m = min_u(n - tot, BSIZE as c_uint - off % BSIZE as c_uint);
        if either_copyin(
            ptr::addr_of_mut!((*bp).data[(off % BSIZE as c_uint) as usize]).cast::<c_void>(),
            user_src,
            src,
            m as u64,
        ) == -1
        {
            brelse(bp);
            break;
        }
        log_write(bp);
        brelse(bp);

        tot = tot.wrapping_add(m);
        off = off.wrapping_add(m);
        src = src.wrapping_add(m as u64);
    }

    if off > (*ip).size {
        (*ip).size = off;
    }
    iupdate(ip);

    tot as c_int
}

#[no_mangle]
pub unsafe extern "C" fn namecmp(s: *const c_char, t: *const c_char) -> c_int {
    strncmp(s, t, DIRSIZ as c_uint)
}

unsafe fn skipelem(mut path: *mut c_char, name: *mut c_char) -> *mut c_char {
    while *path == b'/' as c_char {
        path = path.add(1);
    }
    if *path == 0 {
        return ptr::null_mut();
    }

    let s = path;
    while *path != b'/' as c_char && *path != 0 {
        path = path.add(1);
    }

    let len = path.offset_from(s) as usize;
    if len >= DIRSIZ {
        memmove(name.cast::<c_void>(), s.cast::<c_void>(), DIRSIZ as c_uint);
    } else {
        memmove(name.cast::<c_void>(), s.cast::<c_void>(), len as c_uint);
        *name.add(len) = 0;
    }

    while *path == b'/' as c_char {
        path = path.add(1);
    }

    path
}

#[no_mangle]
pub unsafe extern "C" fn dirlookup(
    dp: *mut Inode,
    name: *mut c_char,
    poff: *mut c_uint,
) -> *mut Inode {
    if (*dp).inode_type as c_int != T_DIR {
        panic(b"dirlookup not DIR\0".as_ptr().cast_mut().cast());
    }

    let mut off: c_uint = 0;
    let mut de: Dirent = mem::zeroed();
    let de_sz_u = mem::size_of::<Dirent>() as c_uint;
    let de_sz_i = de_sz_u as c_int;

    while off < (*dp).size {
        if readi(dp, 0, ptr::addr_of_mut!(de) as u64, off, de_sz_u) != de_sz_i {
            panic(b"dirlookup read\0".as_ptr().cast_mut().cast());
        }
        if de.inum != 0 && strncmp(name, de.name.as_ptr(), DIRSIZ as c_uint) == 0 {
            if !poff.is_null() {
                *poff = off;
            }
            return iget((*dp).dev, de.inum as c_uint);
        }
        off = off.wrapping_add(de_sz_u);
    }

    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn dirlink(dp: *mut Inode, name: *mut c_char, inum: c_uint) -> c_int {
    let ip = dirlookup(dp, name, ptr::null_mut());
    if !ip.is_null() {
        iput(ip);
        return -1;
    }

    let mut off: c_uint = 0;
    let mut de: Dirent = mem::zeroed();
    let de_sz_u = mem::size_of::<Dirent>() as c_uint;
    let de_sz_i = de_sz_u as c_int;

    while off < (*dp).size {
        if readi(dp, 0, ptr::addr_of_mut!(de) as u64, off, de_sz_u) != de_sz_i {
            panic(b"dirlink read\0".as_ptr().cast_mut().cast());
        }
        if de.inum == 0 {
            break;
        }
        off = off.wrapping_add(de_sz_u);
    }

    strncpy(de.name.as_mut_ptr(), name, DIRSIZ as c_int);
    de.inum = inum as u16;
    if writei(dp, 0, ptr::addr_of_mut!(de) as u64, off, de_sz_u) != de_sz_i {
        return -1;
    }

    0
}

unsafe fn namex(mut path: *mut c_char, nameiparent: c_int, name: *mut c_char) -> *mut Inode {
    let mut ip: *mut Inode;

    if *path == b'/' as c_char {
        ip = iget(ROOTDEV, ROOTINO);
    } else {
        ip = idup(myproc_cwd().cast::<Inode>());
    }

    loop {
        path = skipelem(path, name);
        if path.is_null() {
            break;
        }

        ilock(ip);
        if (*ip).inode_type as c_int != T_DIR {
            iunlockput(ip);
            return ptr::null_mut();
        }
        if nameiparent != 0 && *path == 0 {
            iunlock(ip);
            return ip;
        }

        let next = dirlookup(ip, name, ptr::null_mut());
        if next.is_null() {
            iunlockput(ip);
            return ptr::null_mut();
        }

        iunlockput(ip);
        ip = next;
    }

    if nameiparent != 0 {
        iput(ip);
        ptr::null_mut()
    } else {
        ip
    }
}

#[no_mangle]
pub unsafe extern "C" fn namei(path: *mut c_char) -> *mut Inode {
    let mut name = [0 as c_char; DIRSIZ];
    namex(path, 0, name.as_mut_ptr())
}

#[no_mangle]
pub unsafe extern "C" fn nameiparent(path: *mut c_char, name: *mut c_char) -> *mut Inode {
    namex(path, 1, name)
}
