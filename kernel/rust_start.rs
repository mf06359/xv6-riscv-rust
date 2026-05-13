use core::arch::asm;

use crate::rust_main::main;

const NCPU: usize = 8;
const STACK_BYTES: usize = 4096 * NCPU;

const MSTATUS_MPP_MASK: u64 = 3 << 11;
const MSTATUS_MPP_S: u64 = 1 << 11;
const SIE_SEIE: u64 = 1 << 9;
const SIE_STIE: u64 = 1 << 5;
const MIE_STIE: u64 = 1 << 5;
const MENVCFG_STCE: u64 = 1 << 63;
const MCOUNTEREN_TM: u64 = 1 << 1;

#[repr(C, align(16))]
pub struct Stack0([u8; STACK_BYTES]);

#[no_mangle]
pub static mut stack0: Stack0 = Stack0([0; STACK_BYTES]);

#[inline(always)]
unsafe fn r_mhartid() -> u64 {
    let x: u64;
    asm!("csrr {0}, mhartid", out(reg) x);
    x
}

#[inline(always)]
unsafe fn r_mstatus() -> u64 {
    let x: u64;
    asm!("csrr {0}, mstatus", out(reg) x);
    x
}

#[inline(always)]
unsafe fn w_mstatus(x: u64) {
    asm!("csrw mstatus, {0}", in(reg) x);
}

#[inline(always)]
unsafe fn w_mepc(x: u64) {
    asm!("csrw mepc, {0}", in(reg) x);
}

#[inline(always)]
unsafe fn w_satp(x: u64) {
    asm!("csrw satp, {0}", in(reg) x);
}

#[inline(always)]
unsafe fn w_medeleg(x: u64) {
    asm!("csrw medeleg, {0}", in(reg) x);
}

#[inline(always)]
unsafe fn w_mideleg(x: u64) {
    asm!("csrw mideleg, {0}", in(reg) x);
}

#[inline(always)]
unsafe fn r_sie() -> u64 {
    let x: u64;
    asm!("csrr {0}, sie", out(reg) x);
    x
}

#[inline(always)]
unsafe fn w_sie(x: u64) {
    asm!("csrw sie, {0}", in(reg) x);
}

#[inline(always)]
unsafe fn w_pmpaddr0(x: u64) {
    asm!("csrw pmpaddr0, {0}", in(reg) x);
}

#[inline(always)]
unsafe fn w_pmpcfg0(x: u64) {
    asm!("csrw pmpcfg0, {0}", in(reg) x);
}

#[inline(always)]
unsafe fn r_mie() -> u64 {
    let x: u64;
    asm!("csrr {0}, mie", out(reg) x);
    x
}

#[inline(always)]
unsafe fn w_mie(x: u64) {
    asm!("csrw mie, {0}", in(reg) x);
}

#[inline(always)]
unsafe fn r_menvcfg() -> u64 {
    let x: u64;
    asm!("csrr {0}, 0x30a", out(reg) x);
    x
}

#[inline(always)]
unsafe fn w_menvcfg(x: u64) {
    asm!("csrw 0x30a, {0}", in(reg) x);
}

#[inline(always)]
unsafe fn r_mcounteren() -> u64 {
    let x: u64;
    asm!("csrr {0}, mcounteren", out(reg) x);
    x
}

#[inline(always)]
unsafe fn w_mcounteren(x: u64) {
    asm!("csrw mcounteren, {0}", in(reg) x);
}

#[inline(always)]
unsafe fn r_time() -> u64 {
    let x: u64;
    asm!("csrr {0}, time", out(reg) x);
    x
}

#[inline(always)]
unsafe fn w_stimecmp(x: u64) {
    asm!("csrw 0x14d, {0}", in(reg) x);
}

#[inline(always)]
unsafe fn w_tp(x: u64) {
    asm!("mv tp, {0}", in(reg) x);
}

#[no_mangle]
pub unsafe extern "C" fn start() -> ! {
    let mut x = r_mstatus();
    x &= !MSTATUS_MPP_MASK;
    x |= MSTATUS_MPP_S;
    w_mstatus(x);

    w_mepc(main as usize as u64);

    w_satp(0);

    w_medeleg(0xffff);
    w_mideleg(0xffff);
    w_sie(r_sie() | SIE_SEIE | SIE_STIE);

    w_pmpaddr0(0x3fffffffffffffff);
    w_pmpcfg0(0x0f);

    timerinit();

    w_tp(r_mhartid());

    asm!("mret", options(noreturn));
}

#[no_mangle]
pub unsafe extern "C" fn timerinit() {
    w_mie(r_mie() | MIE_STIE);
    w_menvcfg(r_menvcfg() | MENVCFG_STCE);
    w_mcounteren(r_mcounteren() | MCOUNTEREN_TM);
    w_stimecmp(r_time() + 1_000_000);
}
