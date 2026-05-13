use core::ffi::{c_int, c_uint, c_void};
use core::ptr;
use core::sync::atomic::{fence, Ordering};

use crate::rust_bio::Buf;
use crate::rust_kalloc::kalloc;
use crate::rust_printf::panic;
use crate::rust_proc::{sleep, wakeup};
use crate::rust_spinlock::{acquire, initlock, release, Spinlock};
use crate::rust_string::memset;

const BSIZE: usize = 1024;
const PGSIZE: usize = 4096;
const NUM: usize = 8;
const VIRTIO0: usize = 0x1000_1000;

const VIRTIO_MMIO_MAGIC_VALUE: usize = 0x000;
const VIRTIO_MMIO_VERSION: usize = 0x004;
const VIRTIO_MMIO_DEVICE_ID: usize = 0x008;
const VIRTIO_MMIO_VENDOR_ID: usize = 0x00c;
const VIRTIO_MMIO_DEVICE_FEATURES: usize = 0x010;
const VIRTIO_MMIO_DRIVER_FEATURES: usize = 0x020;
const VIRTIO_MMIO_QUEUE_SEL: usize = 0x030;
const VIRTIO_MMIO_QUEUE_NUM_MAX: usize = 0x034;
const VIRTIO_MMIO_QUEUE_NUM: usize = 0x038;
const VIRTIO_MMIO_QUEUE_READY: usize = 0x044;
const VIRTIO_MMIO_QUEUE_NOTIFY: usize = 0x050;
const VIRTIO_MMIO_INTERRUPT_STATUS: usize = 0x060;
const VIRTIO_MMIO_INTERRUPT_ACK: usize = 0x064;
const VIRTIO_MMIO_STATUS: usize = 0x070;
const VIRTIO_MMIO_QUEUE_DESC_LOW: usize = 0x080;
const VIRTIO_MMIO_QUEUE_DESC_HIGH: usize = 0x084;
const VIRTIO_MMIO_DRIVER_DESC_LOW: usize = 0x090;
const VIRTIO_MMIO_DRIVER_DESC_HIGH: usize = 0x094;
const VIRTIO_MMIO_DEVICE_DESC_LOW: usize = 0x0a0;
const VIRTIO_MMIO_DEVICE_DESC_HIGH: usize = 0x0a4;

const VIRTIO_CONFIG_S_ACKNOWLEDGE: u32 = 1;
const VIRTIO_CONFIG_S_DRIVER: u32 = 2;
const VIRTIO_CONFIG_S_DRIVER_OK: u32 = 4;
const VIRTIO_CONFIG_S_FEATURES_OK: u32 = 8;

const VIRTIO_BLK_F_RO: u32 = 5;
const VIRTIO_BLK_F_SCSI: u32 = 7;
const VIRTIO_BLK_F_CONFIG_WCE: u32 = 11;
const VIRTIO_BLK_F_MQ: u32 = 12;
const VIRTIO_F_ANY_LAYOUT: u32 = 27;
const VIRTIO_RING_F_INDIRECT_DESC: u32 = 28;
const VIRTIO_RING_F_EVENT_IDX: u32 = 29;

const VRING_DESC_F_NEXT: u16 = 1;
const VRING_DESC_F_WRITE: u16 = 2;

const VIRTIO_BLK_T_IN: u32 = 0;
const VIRTIO_BLK_T_OUT: u32 = 1;

#[repr(C)]
struct VirtqDesc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

#[repr(C)]
struct VirtqAvail {
    flags: u16,
    idx: u16,
    ring: [u16; NUM],
    unused: u16,
}

#[repr(C)]
struct VirtqUsedElem {
    id: u32,
    len: u32,
}

#[repr(C)]
struct VirtqUsed {
    flags: u16,
    idx: u16,
    ring: [VirtqUsedElem; NUM],
}

#[repr(C)]
#[derive(Copy, Clone)]
struct VirtioBlkReq {
    req_type: u32,
    reserved: u32,
    sector: u64,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct DiskInfo {
    b: *mut Buf,
    status: u8,
}

#[repr(C)]
struct Disk {
    desc: *mut VirtqDesc,
    avail: *mut VirtqAvail,
    used: *mut VirtqUsed,
    free: [u8; NUM],
    used_idx: u16,
    info: [DiskInfo; NUM],
    ops: [VirtioBlkReq; NUM],
    vdisk_lock: Spinlock,
}

const EMPTY_SPINLOCK: Spinlock = Spinlock {
    locked: 0,
    name: ptr::null_mut(),
    cpu: ptr::null_mut(),
};

const EMPTY_INFO: DiskInfo = DiskInfo {
    b: ptr::null_mut(),
    status: 0,
};

const EMPTY_OP: VirtioBlkReq = VirtioBlkReq {
    req_type: 0,
    reserved: 0,
    sector: 0,
};

static mut DISK: Disk = Disk {
    desc: ptr::null_mut(),
    avail: ptr::null_mut(),
    used: ptr::null_mut(),
    free: [0; NUM],
    used_idx: 0,
    info: [EMPTY_INFO; NUM],
    ops: [EMPTY_OP; NUM],
    vdisk_lock: EMPTY_SPINLOCK,
};

#[inline(always)]
unsafe fn info_at(i: usize) -> *mut DiskInfo {
    ptr::addr_of_mut!(DISK.info).cast::<DiskInfo>().add(i)
}

#[inline(always)]
unsafe fn op_at(i: usize) -> *mut VirtioBlkReq {
    ptr::addr_of_mut!(DISK.ops).cast::<VirtioBlkReq>().add(i)
}


#[inline(always)]
unsafe fn mmio_read(off: usize) -> u32 {
    ptr::read_volatile((VIRTIO0 + off) as *const u32)
}

#[inline(always)]
unsafe fn mmio_write(off: usize, val: u32) {
    ptr::write_volatile((VIRTIO0 + off) as *mut u32, val);
}

unsafe fn alloc_desc() -> c_int {
    let mut i = 0usize;
    while i < NUM {
        if DISK.free[i] != 0 {
            DISK.free[i] = 0;
            return i as c_int;
        }
        i += 1;
    }
    -1
}

unsafe fn free_desc(i: c_int) {
    if i < 0 || i as usize >= NUM {
        panic(b"free_desc 1\0".as_ptr().cast_mut().cast());
    }
    if DISK.free[i as usize] != 0 {
        panic(b"free_desc 2\0".as_ptr().cast_mut().cast());
    }

    let d = DISK.desc.add(i as usize);
    (*d).addr = 0;
    (*d).len = 0;
    (*d).flags = 0;
    (*d).next = 0;
    DISK.free[i as usize] = 1;

    wakeup(ptr::addr_of_mut!(DISK.free[0]).cast::<c_void>());
}

unsafe fn free_chain(mut i: c_int) {
    loop {
        let d = DISK.desc.add(i as usize);
        let flag = (*d).flags;
        let nxt = (*d).next as c_int;
        free_desc(i);
        if (flag & VRING_DESC_F_NEXT) != 0 {
            i = nxt;
        } else {
            break;
        }
    }
}

unsafe fn alloc3_desc(idx: &mut [c_int; 3]) -> c_int {
    let mut i = 0usize;
    while i < 3 {
        idx[i] = alloc_desc();
        if idx[i] < 0 {
            let mut j = 0usize;
            while j < i {
                free_desc(idx[j]);
                j += 1;
            }
            return -1;
        }
        i += 1;
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn virtio_disk_init() {
    initlock(
        ptr::addr_of_mut!(DISK.vdisk_lock),
        b"virtio_disk\0".as_ptr().cast_mut().cast(),
    );

    if mmio_read(VIRTIO_MMIO_MAGIC_VALUE) != 0x74726976
        || mmio_read(VIRTIO_MMIO_VERSION) != 2
        || mmio_read(VIRTIO_MMIO_DEVICE_ID) != 2
        || mmio_read(VIRTIO_MMIO_VENDOR_ID) != 0x554d4551
    {
        panic(b"could not find virtio disk\0".as_ptr().cast_mut().cast());
    }

    let mut status: u32 = 0;
    mmio_write(VIRTIO_MMIO_STATUS, status);

    status |= VIRTIO_CONFIG_S_ACKNOWLEDGE;
    mmio_write(VIRTIO_MMIO_STATUS, status);

    status |= VIRTIO_CONFIG_S_DRIVER;
    mmio_write(VIRTIO_MMIO_STATUS, status);

    let mut features = mmio_read(VIRTIO_MMIO_DEVICE_FEATURES);
    features &= !(1 << VIRTIO_BLK_F_RO);
    features &= !(1 << VIRTIO_BLK_F_SCSI);
    features &= !(1 << VIRTIO_BLK_F_CONFIG_WCE);
    features &= !(1 << VIRTIO_BLK_F_MQ);
    features &= !(1 << VIRTIO_F_ANY_LAYOUT);
    features &= !(1 << VIRTIO_RING_F_EVENT_IDX);
    features &= !(1 << VIRTIO_RING_F_INDIRECT_DESC);
    mmio_write(VIRTIO_MMIO_DRIVER_FEATURES, features);

    status |= VIRTIO_CONFIG_S_FEATURES_OK;
    mmio_write(VIRTIO_MMIO_STATUS, status);

    status = mmio_read(VIRTIO_MMIO_STATUS);
    if (status & VIRTIO_CONFIG_S_FEATURES_OK) == 0 {
        panic(b"virtio disk FEATURES_OK unset\0".as_ptr().cast_mut().cast());
    }

    mmio_write(VIRTIO_MMIO_QUEUE_SEL, 0);

    if mmio_read(VIRTIO_MMIO_QUEUE_READY) != 0 {
        panic(b"virtio disk should not be ready\0".as_ptr().cast_mut().cast());
    }

    let max = mmio_read(VIRTIO_MMIO_QUEUE_NUM_MAX);
    if max == 0 {
        panic(b"virtio disk has no queue 0\0".as_ptr().cast_mut().cast());
    }
    if max < NUM as u32 {
        panic(b"virtio disk max queue too short\0".as_ptr().cast_mut().cast());
    }

    DISK.desc = kalloc() as *mut VirtqDesc;
    DISK.avail = kalloc() as *mut VirtqAvail;
    DISK.used = kalloc() as *mut VirtqUsed;
    if DISK.desc.is_null() || DISK.avail.is_null() || DISK.used.is_null() {
        panic(b"virtio disk kalloc\0".as_ptr().cast_mut().cast());
    }
    memset(DISK.desc.cast::<c_void>(), 0, PGSIZE as c_uint);
    memset(DISK.avail.cast::<c_void>(), 0, PGSIZE as c_uint);
    memset(DISK.used.cast::<c_void>(), 0, PGSIZE as c_uint);

    mmio_write(VIRTIO_MMIO_QUEUE_NUM, NUM as u32);

    let desc_pa = DISK.desc as u64;
    mmio_write(VIRTIO_MMIO_QUEUE_DESC_LOW, desc_pa as u32);
    mmio_write(VIRTIO_MMIO_QUEUE_DESC_HIGH, (desc_pa >> 32) as u32);

    let avail_pa = DISK.avail as u64;
    mmio_write(VIRTIO_MMIO_DRIVER_DESC_LOW, avail_pa as u32);
    mmio_write(VIRTIO_MMIO_DRIVER_DESC_HIGH, (avail_pa >> 32) as u32);

    let used_pa = DISK.used as u64;
    mmio_write(VIRTIO_MMIO_DEVICE_DESC_LOW, used_pa as u32);
    mmio_write(VIRTIO_MMIO_DEVICE_DESC_HIGH, (used_pa >> 32) as u32);

    mmio_write(VIRTIO_MMIO_QUEUE_READY, 1);

    let mut i = 0usize;
    while i < NUM {
        DISK.free[i] = 1;
        i += 1;
    }

    status |= VIRTIO_CONFIG_S_DRIVER_OK;
    mmio_write(VIRTIO_MMIO_STATUS, status);
}

#[no_mangle]
pub unsafe extern "C" fn virtio_disk_rw(b: *mut Buf, write: c_int) {
    let sector = (*b).blockno as u64 * (BSIZE as u64 / 512);

    acquire(ptr::addr_of_mut!(DISK.vdisk_lock));

    let mut idx = [-1; 3];
    loop {
        if alloc3_desc(&mut idx) == 0 {
            break;
        }
        sleep(
            ptr::addr_of_mut!(DISK.free[0]).cast::<c_void>(),
            ptr::addr_of_mut!(DISK.vdisk_lock),
        );
    }

    let buf0 = op_at(idx[0] as usize);
    if write != 0 {
        (*buf0).req_type = VIRTIO_BLK_T_OUT;
    } else {
        (*buf0).req_type = VIRTIO_BLK_T_IN;
    }
    (*buf0).reserved = 0;
    (*buf0).sector = sector;

    let d0 = DISK.desc.add(idx[0] as usize);
    (*d0).addr = buf0 as u64;
    (*d0).len = core::mem::size_of::<VirtioBlkReq>() as u32;
    (*d0).flags = VRING_DESC_F_NEXT;
    (*d0).next = idx[1] as u16;

    let d1 = DISK.desc.add(idx[1] as usize);
    (*d1).addr = ptr::addr_of_mut!((*b).data).cast::<u8>() as u64;
    (*d1).len = BSIZE as u32;
    if write != 0 {
        (*d1).flags = 0;
    } else {
        (*d1).flags = VRING_DESC_F_WRITE;
    }
    (*d1).flags |= VRING_DESC_F_NEXT;
    (*d1).next = idx[2] as u16;

    (*info_at(idx[0] as usize)).status = 0xff;
    let d2 = DISK.desc.add(idx[2] as usize);
    (*d2).addr = ptr::addr_of_mut!((*info_at(idx[0] as usize)).status) as u64;
    (*d2).len = 1;
    (*d2).flags = VRING_DESC_F_WRITE;
    (*d2).next = 0;

    (*b).disk = 1;
    (*info_at(idx[0] as usize)).b = b;

    ptr::addr_of_mut!((*DISK.avail).ring)
        .cast::<u16>()
        .add(((*DISK.avail).idx as usize) % NUM)
        .write(idx[0] as u16);

    fence(Ordering::SeqCst);

    (*DISK.avail).idx = (*DISK.avail).idx.wrapping_add(1);

    fence(Ordering::SeqCst);

    mmio_write(VIRTIO_MMIO_QUEUE_NOTIFY, 0);

    while (*b).disk == 1 {
        sleep(b.cast::<c_void>(), ptr::addr_of_mut!(DISK.vdisk_lock));
    }

    (*info_at(idx[0] as usize)).b = ptr::null_mut();
    free_chain(idx[0]);

    release(ptr::addr_of_mut!(DISK.vdisk_lock));
}

#[no_mangle]
pub unsafe extern "C" fn virtio_disk_intr() {
    acquire(ptr::addr_of_mut!(DISK.vdisk_lock));

    let st = mmio_read(VIRTIO_MMIO_INTERRUPT_STATUS);
    mmio_write(VIRTIO_MMIO_INTERRUPT_ACK, st & 0x3);

    fence(Ordering::SeqCst);

    while DISK.used_idx != (*DISK.used).idx {
        fence(Ordering::SeqCst);

        let id = ptr::addr_of!((*DISK.used).ring)
            .cast::<VirtqUsedElem>()
            .add((DISK.used_idx as usize) % NUM)
            .read()
            .id as usize;

        if (*info_at(id)).status != 0 {
            panic(b"virtio_disk_intr status\0".as_ptr().cast_mut().cast());
        }

        let b = (*info_at(id)).b;
        (*b).disk = 0;
        wakeup(b.cast::<c_void>());

        DISK.used_idx = DISK.used_idx.wrapping_add(1);
    }

    release(ptr::addr_of_mut!(DISK.vdisk_lock));
}
