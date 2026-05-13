use core::ffi::{c_char, c_int};
use core::ptr;

use crate::rust_console::consoleintr;
use crate::rust_lock::{wake, SpinMutex};
use crate::rust_printf::{panicked, panicking};
use crate::rust_spinlock::InterruptGuard;

const UART0: usize = 0x1000_0000;

const RHR: usize = 0;
const THR: usize = 0;
const IER: usize = 1;
const IER_RX_ENABLE: u8 = 1 << 0;
const IER_TX_ENABLE: u8 = 1 << 1;
const FCR: usize = 2;
const FCR_FIFO_ENABLE: u8 = 1 << 0;
const FCR_FIFO_CLEAR: u8 = 3 << 1;
#[allow(dead_code)]
const ISR: usize = 2;
const LCR: usize = 3;
const LCR_EIGHT_BITS: u8 = 3 << 0;
const LCR_BAUD_LATCH: u8 = 1 << 7;
const LSR: usize = 5;
const LSR_RX_READY: u8 = 1 << 0;
const LSR_TX_IDLE: u8 = 1 << 5;

const TX_BUF_SIZE: usize = 256;

/// Per-UART transmit ring. Producers (`uartwrite`) push bytes into the
/// ring and call `start_tx_locked`; the TX-empty interrupt drains it.
struct UartTx {
    buf: [u8; TX_BUF_SIZE],
    /// Total bytes ever written into the ring (monotonic).
    w: u64,
    /// Total bytes ever consumed by the UART (monotonic).
    r: u64,
}

impl UartTx {
    const fn new() -> Self {
        Self {
            buf: [0; TX_BUF_SIZE],
            w: 0,
            r: 0,
        }
    }
}

static UART: SpinMutex<UartTx> = SpinMutex::new(UartTx::new(), b"uart\0");

#[inline(always)]
unsafe fn reg_ptr(reg: usize) -> *mut u8 {
    (UART0 + reg) as *mut u8
}

#[inline(always)]
unsafe fn read_reg(reg: usize) -> u8 {
    ptr::read_volatile(reg_ptr(reg) as *const u8)
}

#[inline(always)]
unsafe fn write_reg(reg: usize, v: u8) {
    ptr::write_volatile(reg_ptr(reg), v);
}

#[no_mangle]
pub unsafe extern "C" fn uartinit() {
    write_reg(IER, 0x00);
    write_reg(LCR, LCR_BAUD_LATCH);
    write_reg(0, 0x03);
    write_reg(1, 0x00);
    write_reg(LCR, LCR_EIGHT_BITS);
    write_reg(FCR, FCR_FIFO_ENABLE | FCR_FIFO_CLEAR);
    write_reg(IER, IER_TX_ENABLE | IER_RX_ENABLE);

    UART.init();
}

/// Drain the TX ring into the UART as long as the device is willing to
/// accept bytes. Caller must hold `UART`.
unsafe fn start_tx_locked(tx: &mut UartTx) {
    loop {
        if tx.w == tx.r {
            return;
        }
        if (read_reg(LSR) & LSR_TX_IDLE) == 0 {
            // UART hardware is busy; the TX-empty interrupt will call us back.
            return;
        }
        let idx = (tx.r as usize) % TX_BUF_SIZE;
        let c = tx.buf[idx];
        tx.r = tx.r.wrapping_add(1);
        // Wake any producer waiting for space.
        wake(UART.chan());
        write_reg(THR, c);
    }
}

#[no_mangle]
pub unsafe extern "C" fn uartwrite(buf: *mut c_char, n: c_int) {
    let chan = UART.chan();
    let mut tx = UART.lock();

    let mut i = 0;
    while i < n {
        // Wait while the ring is full.
        while tx.w.wrapping_sub(tx.r) == TX_BUF_SIZE as u64 {
            start_tx_locked(&mut tx);
            tx.sleep(chan);
        }
        let idx = (tx.w as usize) % TX_BUF_SIZE;
        tx.buf[idx] = *buf.add(i as usize) as u8;
        tx.w = tx.w.wrapping_add(1);
        i += 1;
    }
    start_tx_locked(&mut tx);
}

#[no_mangle]
pub unsafe extern "C" fn uartputc_sync(c: c_int) {
    let _irq = if panicking == 0 {
        Some(InterruptGuard::new())
    } else {
        None
    };

    if panicked != 0 {
        loop {
            core::hint::spin_loop();
        }
    }

    while (read_reg(LSR) & LSR_TX_IDLE) == 0 {}
    write_reg(THR, c as u8);
    drop(_irq);
}

#[no_mangle]
pub unsafe extern "C" fn uartgetc() -> c_int {
    if (read_reg(LSR) & LSR_RX_READY) != 0 {
        read_reg(RHR) as c_int
    } else {
        -1
    }
}

#[no_mangle]
pub unsafe extern "C" fn uartintr() {
    // Drain the RX FIFO first.
    loop {
        let c = uartgetc();
        if c == -1 {
            break;
        }
        consoleintr(c);
    }

    // Then push more bytes if the device is idle.
    let mut tx = UART.lock();
    start_tx_locked(&mut tx);
}
