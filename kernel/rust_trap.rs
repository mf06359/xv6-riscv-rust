use core::ffi::{c_int, c_uint, c_void};
use core::ptr;

use crate::rust_plic::{plic_claim, plic_complete};
use crate::rust_printf::{kprintf1, kprintf2, kprintf3, panic};
use crate::rust_proc::{killed, kexit, myproc, cpuid, setkilled, wakeup, Proc};
use crate::rust_spinlock::{acquire, initlock, release, Spinlock};
use crate::rust_syscall::syscall;
use crate::rust_uart::uartintr;
use crate::rust_virtio_disk::virtio_disk_intr;
use crate::rust_vm::vmfault;

const UART0_IRQ: c_int = 10;
const VIRTIO0_IRQ: c_int = 1;
const SCAUSE_SEXT: u64 = 0x8000_0000_0000_0009;
const SCAUSE_STIMER: u64 = 0x8000_0000_0000_0005;
const TIMER_INTERVAL: u64 = 1_000_000;
const PGSIZE: u64 = 4096;
const MAXVA: u64 = 1u64 << (9 + 9 + 9 + 12 - 1);
const TRAMPOLINE: u64 = MAXVA - PGSIZE;
const SATP_SV39: u64 = 8u64 << 60;
const SSTATUS_SIE: u64 = 1 << 1;
const SSTATUS_SPIE: u64 = 1 << 5;
const SSTATUS_SPP: u64 = 1 << 8;

#[no_mangle]
pub static mut tickslock: Spinlock = Spinlock {
    locked: 0,
    name: ptr::null_mut(),
    cpu: ptr::null_mut(),
};

#[no_mangle]
pub static mut ticks: c_uint = 0;

extern "C" {
    static trampoline: u8;
    static uservec: u8;
    fn kernelvec();
}

#[inline(always)]
fn make_satp(pagetable: *mut u64) -> u64 {
    SATP_SV39 | ((pagetable as u64) >> 12)
}

#[inline(always)]
unsafe fn r_scause() -> u64 {
    let mut x: u64;
    core::arch::asm!("csrr {0}, scause", out(reg) x);
    x
}

#[inline(always)]
unsafe fn r_stval() -> u64 {
    let mut x: u64;
    core::arch::asm!("csrr {0}, stval", out(reg) x);
    x
}

#[inline(always)]
unsafe fn r_time() -> u64 {
    let mut x: u64;
    core::arch::asm!("csrr {0}, time", out(reg) x);
    x
}

#[inline(always)]
unsafe fn r_sepc() -> u64 {
    let mut x: u64;
    core::arch::asm!("csrr {0}, sepc", out(reg) x);
    x
}

#[inline(always)]
unsafe fn w_stimecmp(x: u64) {
    core::arch::asm!("csrw 0x14d, {0}", in(reg) x);
}

#[inline(always)]
unsafe fn w_stvec(x: u64) {
    core::arch::asm!("csrw stvec, {0}", in(reg) x);
}

#[inline(always)]
unsafe fn r_satp() -> u64 {
    let mut x: u64;
    core::arch::asm!("csrr {0}, satp", out(reg) x);
    x
}

#[inline(always)]
unsafe fn r_tp() -> u64 {
    let x: u64;
    core::arch::asm!("mv {0}, tp", out(reg) x);
    x
}

#[inline(always)]
unsafe fn r_sstatus() -> u64 {
    let mut x: u64;
    core::arch::asm!("csrr {0}, sstatus", out(reg) x);
    x
}

#[inline(always)]
unsafe fn w_sstatus(x: u64) {
    core::arch::asm!("csrw sstatus, {0}", in(reg) x);
}

#[inline(always)]
unsafe fn w_sepc(x: u64) {
    core::arch::asm!("csrw sepc, {0}", in(reg) x);
}

#[inline(always)]
unsafe fn intr_on() {
    let mut x: u64;
    core::arch::asm!("csrr {0}, sstatus", out(reg) x);
    x |= SSTATUS_SIE;
    core::arch::asm!("csrw sstatus, {0}", in(reg) x);
}

#[inline(always)]
unsafe fn intr_off() {
    let mut x: u64;
    core::arch::asm!("csrr {0}, sstatus", out(reg) x);
    x &= !SSTATUS_SIE;
    core::arch::asm!("csrw sstatus, {0}", in(reg) x);
}

#[inline(always)]
unsafe fn intr_get() -> c_int {
    ((r_sstatus() & SSTATUS_SIE) != 0) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn trapinit() {
    initlock(ptr::addr_of_mut!(tickslock), b"time\0".as_ptr().cast_mut().cast());
}

#[no_mangle]
pub unsafe extern "C" fn trapinithart() {
    w_stvec(kernelvec as usize as u64);
}

#[no_mangle]
pub unsafe extern "C" fn prepare_return() {
    let p = myproc();
    if p.is_null() {
        panic(b"prepare_return: no proc\0".as_ptr().cast_mut().cast());
    }

    intr_off();

    let trampoline_uservec = TRAMPOLINE
        + ((ptr::addr_of!(uservec) as u64).wrapping_sub(ptr::addr_of!(trampoline) as u64));
    w_stvec(trampoline_uservec);

    let tf = (*p).trapframe;
    (*tf).kernel_satp = r_satp();
    (*tf).kernel_sp = (*p).kstack + PGSIZE;
    (*tf).kernel_trap = usertrap as usize as u64;
    (*tf).kernel_hartid = r_tp();

    let mut x = r_sstatus();
    x &= !SSTATUS_SPP;
    x |= SSTATUS_SPIE;
    w_sstatus(x);

    w_sepc((*tf).epc);
}

#[no_mangle]
pub unsafe extern "C" fn usertrap() -> u64 {
    if (r_sstatus() & SSTATUS_SPP) != 0 {
        panic(
            b"usertrap: not from user mode\0"
                .as_ptr()
                .cast_mut()
                .cast(),
        );
    }

    w_stvec(kernelvec as usize as u64);

    let p = myproc();
    if p.is_null() {
        panic(b"usertrap: no proc\0".as_ptr().cast_mut().cast());
    }

    let tf = (*p).trapframe;
    (*tf).epc = r_sepc();
    let scause = r_scause();
    let stval = r_stval();

    let which_dev = usertrap_dispatch(
        p,
        ptr::addr_of_mut!((*tf).epc),
        (*p).pid,
        (*p).pagetable,
        scause,
        stval,
    );

    if killed(p) != 0 {
        kexit(-1);
    }

    if which_dev == 2 {
        crate::rust_proc::r#yield();
    }

    prepare_return();

    make_satp((*p).pagetable)
}

#[no_mangle]
pub unsafe extern "C" fn kerneltrap() {
    let sepc = r_sepc();
    let sstatus = r_sstatus();
    let scause = r_scause();
    let stval = r_stval();

    if (sstatus & SSTATUS_SPP) == 0 {
        panic(
            b"kerneltrap: not from supervisor mode\0"
                .as_ptr()
                .cast_mut()
                .cast(),
        );
    }
    if intr_get() != 0 {
        panic(
            b"kerneltrap: interrupts enabled\0"
                .as_ptr()
                .cast_mut()
                .cast(),
        );
    }

    let which_dev = kerneltrap_dispatch(scause, sepc, stval);

    if which_dev == 2 && !myproc().is_null() {
        crate::rust_proc::r#yield();
    }

    w_sepc(sepc);
    w_sstatus(sstatus);
}

#[no_mangle]
pub unsafe extern "C" fn usertrap_dispatch(
    p: *mut Proc,
    epc_slot: *mut u64,
    pid: c_int,
    pagetable: *mut u64,
    scause: u64,
    stval: u64,
) -> c_int {
    if scause == 8 {
        if killed(p) != 0 {
            kexit(-1);
        }
        *epc_slot = (*epc_slot).wrapping_add(4);
        intr_on();
        syscall();
        return 0;
    }

    let which_dev = devintr();
    if which_dev != 0 {
        return which_dev;
    }

    // scause 13 = load page fault, scause 15 = store/AMO page fault.
    // Pass is_write=1 for store faults so the COW handler in `vmfault`
    // knows when to do the page copy.
    let is_pf = scause == 15 || scause == 13;
    if is_pf && vmfault(pagetable, stval, if scause == 15 { 1 } else { 0 }) != 0 {
        return 0;
    }

    kprintf2(
        b"usertrap(): unexpected scause 0x%lx pid=%d\n\0"
            .as_ptr()
            .cast(),
        scause,
        pid as u64,
    );
    kprintf2(
        b"            sepc=0x%lx stval=0x%lx\n\0"
            .as_ptr()
            .cast(),
        *epc_slot,
        stval,
    );
    setkilled(p);
    0
}

#[no_mangle]
pub unsafe extern "C" fn kerneltrap_dispatch(scause: u64, sepc: u64, stval: u64) -> c_int {
    let which_dev = devintr();
    if which_dev == 0 {
        kprintf3(
            b"scause=0x%lx sepc=0x%lx stval=0x%lx\n\0"
                .as_ptr()
                .cast(),
            scause,
            sepc,
            stval,
        );
        panic(b"kerneltrap\0".as_ptr().cast_mut().cast());
    }
    which_dev
}

#[no_mangle]
pub unsafe extern "C" fn clockintr() {
    if cpuid() == 0 {
        acquire(ptr::addr_of_mut!(tickslock));
        ticks = ticks.wrapping_add(1);
        wakeup(ptr::addr_of_mut!(ticks).cast::<c_void>());
        release(ptr::addr_of_mut!(tickslock));
    }

    w_stimecmp(r_time() + TIMER_INTERVAL);
}

#[no_mangle]
pub unsafe extern "C" fn devintr() -> c_int {
    let scause = r_scause();

    if scause == SCAUSE_SEXT {
        let irq = plic_claim();

        if irq == UART0_IRQ {
            uartintr();
        } else if irq == VIRTIO0_IRQ {
            virtio_disk_intr();
        } else if irq != 0 {
            kprintf1(b"unexpected interrupt irq=%d\n\0".as_ptr().cast(), irq as u64);
        }

        if irq != 0 {
            plic_complete(irq);
        }
        1
    } else if scause == SCAUSE_STIMER {
        clockintr();
        2
    } else {
        0
    }
}
