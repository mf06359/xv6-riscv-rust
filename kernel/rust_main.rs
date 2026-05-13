use core::ffi::{c_char, c_int};
use core::ptr;
use core::sync::atomic::{fence, Ordering};

use crate::rust_bio::binit;
use crate::rust_console::{consoleinit, consputc};
use crate::rust_file::fileinit;
use crate::rust_fs::iinit;
use crate::rust_kalloc::kinit;
use crate::rust_plic::{plicinit, plicinithart};
use crate::rust_printf::{printfinit, rust_printint};
use crate::rust_proc::{cpuid, procinit, scheduler, userinit};
use crate::rust_spinlock::{acquire, initlock, release, Spinlock};
use crate::rust_trap::{trapinit, trapinithart};
use crate::rust_virtio_disk::virtio_disk_init;
use crate::rust_vm::{kvminit, kvminithart};

static mut STARTED: c_int = 0;

static mut PRINT_LOCK: Spinlock = Spinlock {
    locked: 0,
    name: ptr::null_mut(),
    cpu: ptr::null_mut(),
};

static NL: &[u8] = b"\n";
static BOOT_MSG: &[u8] = b"xv6 kernel is booting\n";
static HART_PREFIX: &[u8] = b"hart ";
static HART_SUFFIX: &[u8] = b" starting\n";
static PRINT_LOCK_NAME: &[u8] = b"main-print\0";

#[inline(always)]
unsafe fn started_load() -> c_int {
    ptr::read_volatile(ptr::addr_of!(STARTED))
}

#[inline(always)]
unsafe fn started_store(v: c_int) {
    ptr::write_volatile(ptr::addr_of_mut!(STARTED), v);
}

unsafe fn print_bytes(bytes: &[u8]) {
    let mut i = 0usize;
    while i < bytes.len() {
        consputc(bytes[i] as c_int);
        i += 1;
    }
}

#[no_mangle]
pub unsafe extern "C" fn main() {
    if cpuid() == 0 {
        consoleinit();
        printfinit();
        initlock(
            ptr::addr_of_mut!(PRINT_LOCK),
            PRINT_LOCK_NAME.as_ptr().cast::<c_char>() as *mut c_char,
        );
        acquire(ptr::addr_of_mut!(PRINT_LOCK));
        print_bytes(NL);
        print_bytes(BOOT_MSG);
        print_bytes(NL);
        release(ptr::addr_of_mut!(PRINT_LOCK));

        kinit();
        kvminit();
        kvminithart();
        procinit();
        trapinit();
        trapinithart();
        plicinit();
        plicinithart();
        binit();
        iinit();
        fileinit();
        virtio_disk_init();
        userinit();

        fence(Ordering::SeqCst);
        started_store(1);
    } else {
        while started_load() == 0 {
            core::hint::spin_loop();
        }
        fence(Ordering::SeqCst);
        acquire(ptr::addr_of_mut!(PRINT_LOCK));
        print_bytes(HART_PREFIX);
        rust_printint(cpuid() as i64, 10, 1);
        print_bytes(HART_SUFFIX);
        release(ptr::addr_of_mut!(PRINT_LOCK));
        kvminithart();
        trapinithart();
        plicinithart();
    }

    scheduler();
}
