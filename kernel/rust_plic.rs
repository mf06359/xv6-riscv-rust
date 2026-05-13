use core::ffi::c_int;
use core::ptr;

use crate::rust_proc::cpuid;

const UART0_IRQ: usize = 10;
const VIRTIO0_IRQ: usize = 1;

const PLIC: usize = 0x0c00_0000;
const PLIC_SENABLE_BASE: usize = PLIC + 0x2080;
const PLIC_SPRIORITY_BASE: usize = PLIC + 0x201000;
const PLIC_SCLAIM_BASE: usize = PLIC + 0x201004;
const PLIC_SENABLE_STRIDE: usize = 0x100;
const PLIC_CONTEXT_STRIDE: usize = 0x2000;

#[inline(always)]
fn plic_priority_reg(irq: usize) -> *mut u32 {
    (PLIC + irq * 4) as *mut u32
}

#[inline(always)]
fn plic_senable_reg(hart: usize) -> *mut u32 {
    (PLIC_SENABLE_BASE + hart * PLIC_SENABLE_STRIDE) as *mut u32
}

#[inline(always)]
fn plic_spriority_reg(hart: usize) -> *mut u32 {
    (PLIC_SPRIORITY_BASE + hart * PLIC_CONTEXT_STRIDE) as *mut u32
}

#[inline(always)]
fn plic_sclaim_reg(hart: usize) -> *mut u32 {
    (PLIC_SCLAIM_BASE + hart * PLIC_CONTEXT_STRIDE) as *mut u32
}

#[no_mangle]
pub unsafe extern "C" fn plicinit() {
    ptr::write_volatile(plic_priority_reg(UART0_IRQ), 1);
    ptr::write_volatile(plic_priority_reg(VIRTIO0_IRQ), 1);
}

#[no_mangle]
pub unsafe extern "C" fn plicinithart() {
    let hart = cpuid() as usize;
    let enables = (1u32 << UART0_IRQ) | (1u32 << VIRTIO0_IRQ);
    ptr::write_volatile(plic_senable_reg(hart), enables);
    ptr::write_volatile(plic_spriority_reg(hart), 0);
}

#[no_mangle]
pub unsafe extern "C" fn plic_claim() -> c_int {
    let hart = cpuid() as usize;
    ptr::read_volatile(plic_sclaim_reg(hart)) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn plic_complete(irq: c_int) {
    let hart = cpuid() as usize;
    ptr::write_volatile(plic_sclaim_reg(hart), irq as u32);
}
