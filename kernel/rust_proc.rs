use core::ffi::{c_char, c_int, c_uint, c_void};
use core::ptr;
use core::sync::atomic::{fence, Ordering};

use crate::rust_console::consputc;
use crate::rust_exec::kexec;
use crate::rust_file::{fileclose, filedup, File};
use crate::rust_fs::{fsinit, idup, iput, namei, Inode};
use crate::rust_kalloc::{kalloc, kfree};
use crate::rust_log::TxnGuard;
use crate::rust_printf::{panic, rust_printint};
use crate::rust_spinlock::{
    acquire, holding, initlock, intr_off, intr_on, release, Context, Cpu, InterruptGuard,
    Spinlock,
};
use crate::rust_string::{memmove, safestrcpy};
use crate::rust_syscall::{argaddr, argint};
use crate::rust_trap::prepare_return;
use crate::rust_vm::{
    copyin, copyout, kvmmap, mappages, uvmalloc, uvmcopy, uvmcreate, uvmdealloc, uvmfree, uvmunmap,
};

extern "C" {
    static trampoline: u8;
    static userret: u8;
    fn swtch(old: *mut Context, new: *mut Context);
}

const NCPU: usize = 8;
const NPROC: usize = 64;
const NOFILE: usize = 16;
const UNUSED: c_int = 0;
const USED: c_int = 1;
const SLEEPING: c_int = 2;
const RUNNABLE: c_int = 3;
const RUNNING: c_int = 4;
const ZOMBIE: c_int = 5;
const ROOTDEV: c_int = 1;
const PGSIZE: u64 = 4096;
const MAXVA: u64 = 1u64 << (9 + 9 + 9 + 12 - 1);
const TRAMPOLINE: u64 = MAXVA - PGSIZE;
const TRAPFRAME: u64 = TRAMPOLINE - PGSIZE;
const SATP_SV39: u64 = 8u64 << 60;
const PTE_R: c_int = 1 << 1;
const PTE_W: c_int = 1 << 2;
const PTE_X: c_int = 1 << 3;
const SBRK_EAGER: c_int = 1;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Trapframe {
    pub kernel_satp: u64,
    pub kernel_sp: u64,
    pub kernel_trap: u64,
    pub epc: u64,
    pub kernel_hartid: u64,
    pub ra: u64,
    pub sp: u64,
    pub gp: u64,
    pub tp: u64,
    pub t0: u64,
    pub t1: u64,
    pub t2: u64,
    pub s0: u64,
    pub s1: u64,
    pub a0: u64,
    pub a1: u64,
    pub a2: u64,
    pub a3: u64,
    pub a4: u64,
    pub a5: u64,
    pub a6: u64,
    pub a7: u64,
    pub s2: u64,
    pub s3: u64,
    pub s4: u64,
    pub s5: u64,
    pub s6: u64,
    pub s7: u64,
    pub s8: u64,
    pub s9: u64,
    pub s10: u64,
    pub s11: u64,
    pub t3: u64,
    pub t4: u64,
    pub t5: u64,
    pub t6: u64,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Proc {
    pub lock: Spinlock,
    pub state: c_int,
    pub chan: *mut c_void,
    pub killed: c_int,
    pub xstate: c_int,
    pub pid: c_int,
    pub parent: *mut Proc,
    pub kstack: u64,
    pub sz: u64,
    pub pagetable: *mut u64,
    pub trapframe: *mut Trapframe,
    pub context: Context,
    pub ofile: [*mut c_void; NOFILE],
    pub cwd: *mut c_void,
    pub name: [i8; 16],
}

static mut FORKRET_FIRST: c_int = 1;

const EMPTY_SPINLOCK: Spinlock = Spinlock {
    locked: 0,
    name: ptr::null_mut(),
    cpu: ptr::null_mut(),
};

const EMPTY_CONTEXT: Context = Context {
    ra: 0,
    sp: 0,
    s0: 0,
    s1: 0,
    s2: 0,
    s3: 0,
    s4: 0,
    s5: 0,
    s6: 0,
    s7: 0,
    s8: 0,
    s9: 0,
    s10: 0,
    s11: 0,
};

const EMPTY_CPU: Cpu = Cpu {
    proc: ptr::null_mut(),
    context: EMPTY_CONTEXT,
    noff: 0,
    intena: 0,
};

const EMPTY_PROC: Proc = Proc {
    lock: EMPTY_SPINLOCK,
    state: UNUSED,
    chan: ptr::null_mut(),
    killed: 0,
    xstate: 0,
    pid: 0,
    parent: ptr::null_mut(),
    kstack: 0,
    sz: 0,
    pagetable: ptr::null_mut(),
    trapframe: ptr::null_mut(),
    context: EMPTY_CONTEXT,
    ofile: [ptr::null_mut(); NOFILE],
    cwd: ptr::null_mut(),
    name: [0; 16],
};

#[no_mangle]
pub static mut cpus: [Cpu; NCPU] = [EMPTY_CPU; NCPU];

#[no_mangle]
pub static mut proc: [Proc; NPROC] = [EMPTY_PROC; NPROC];

#[no_mangle]
pub static mut initproc: *mut Proc = ptr::null_mut();

#[no_mangle]
pub static mut nextpid: c_int = 1;

#[no_mangle]
pub static mut pid_lock: Spinlock = EMPTY_SPINLOCK;

#[no_mangle]
pub static mut wait_lock: Spinlock = EMPTY_SPINLOCK;

#[inline(always)]
fn make_satp(pagetable: *mut u64) -> u64 {
    SATP_SV39 | ((pagetable as u64) >> 12)
}

#[inline(always)]
unsafe fn dump_putc(ch: u8) {
    consputc(ch as c_int);
}

unsafe fn dump_bytes(bytes: &[u8]) {
    let mut i = 0usize;
    while i < bytes.len() {
        dump_putc(bytes[i]);
        i += 1;
    }
}

unsafe fn dump_name(name: &[i8; 16]) {
    let mut i = 0usize;
    while i < name.len() {
        let ch = name[i] as u8;
        if ch == 0 {
            break;
        }
        dump_putc(ch);
        i += 1;
    }
}

#[inline(always)]
fn kstack(i: usize) -> u64 {
    TRAMPOLINE - ((i as u64 + 1) * 2 * PGSIZE)
}

#[no_mangle]
pub unsafe extern "C" fn proc_mapstacks(kpgtbl: *mut u64) {
    let mut i = 0usize;
    let base = ptr::addr_of_mut!(proc).cast::<Proc>();
    while i < NPROC {
        let p = base.add(i);
        let pa = kalloc();
        if pa.is_null() {
            panic(b"kalloc\0".as_ptr() as *mut c_char);
        }
        let va = kstack(i);
        kvmmap(kpgtbl, va, pa as u64, PGSIZE, PTE_R | PTE_W);
        (*p).kstack = va;
        i += 1;
    }
}

unsafe fn allocproc() -> *mut Proc {
    let mut i = 0usize;
    let base = ptr::addr_of_mut!(proc).cast::<Proc>();

    while i < NPROC {
        let p = base.add(i);
        acquire(ptr::addr_of_mut!((*p).lock));
        if (*p).state == UNUSED {
            (*p).pid = allocpid();
            (*p).state = USED;

            (*p).trapframe = kalloc() as *mut Trapframe;
            if (*p).trapframe.is_null() {
                freeproc_locked(p);
                release(ptr::addr_of_mut!((*p).lock));
                return ptr::null_mut();
            }

            (*p).pagetable = proc_pagetable(p);
            if (*p).pagetable.is_null() {
                freeproc_locked(p);
                release(ptr::addr_of_mut!((*p).lock));
                return ptr::null_mut();
            }

            (*p).context = EMPTY_CONTEXT;
            (*p).context.ra = forkret as usize as u64;
            (*p).context.sp = (*p).kstack + PGSIZE;
            return p;
        }
        release(ptr::addr_of_mut!((*p).lock));
        i += 1;
    }

    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn proc_pagetable(p: *mut Proc) -> *mut u64 {
    let pagetable = uvmcreate();
    if pagetable.is_null() {
        return ptr::null_mut();
    }

    if mappages(
        pagetable,
        TRAMPOLINE,
        PGSIZE,
        ptr::addr_of!(trampoline) as u64,
        PTE_R | PTE_X,
    ) < 0
    {
        uvmfree(pagetable, 0);
        return ptr::null_mut();
    }

    if mappages(
        pagetable,
        TRAPFRAME,
        PGSIZE,
        (*p).trapframe as u64,
        PTE_R | PTE_W,
    ) < 0
    {
        uvmunmap(pagetable, TRAMPOLINE, 1, 0);
        uvmfree(pagetable, 0);
        return ptr::null_mut();
    }

    pagetable
}

#[no_mangle]
pub unsafe extern "C" fn proc_freepagetable(pagetable: *mut u64, sz: u64) {
    uvmunmap(pagetable, TRAMPOLINE, 1, 0);
    uvmunmap(pagetable, TRAPFRAME, 1, 0);
    uvmfree(pagetable, sz);
}

#[no_mangle]
pub unsafe extern "C" fn kfork() -> c_int {
    let p = myproc();
    let np = allocproc();
    if np.is_null() {
        return -1;
    }

    if uvmcopy((*p).pagetable, (*np).pagetable, (*p).sz) < 0 {
        freeproc_locked(np);
        release(ptr::addr_of_mut!((*np).lock));
        return -1;
    }
    (*np).sz = (*p).sz;

    *(*np).trapframe = *(*p).trapframe;
    (*(*np).trapframe).a0 = 0;

    let mut i = 0usize;
    while i < NOFILE {
        if !(*p).ofile[i].is_null() {
            (*np).ofile[i] = filedup((*p).ofile[i].cast::<File>()).cast::<c_void>();
        }
        i += 1;
    }
    (*np).cwd = idup((*p).cwd.cast::<Inode>()).cast::<c_void>();

    safestrcpy(
        (*np).name.as_mut_ptr().cast::<c_char>(),
        (*p).name.as_ptr().cast::<c_char>(),
        (*p).name.len() as c_int,
    );

    let pid = (*np).pid;

    release(ptr::addr_of_mut!((*np).lock));

    acquire(ptr::addr_of_mut!(wait_lock));
    (*np).parent = p;
    release(ptr::addr_of_mut!(wait_lock));

    acquire(ptr::addr_of_mut!((*np).lock));
    (*np).state = RUNNABLE;
    release(ptr::addr_of_mut!((*np).lock));

    pid
}

#[no_mangle]
pub unsafe extern "C" fn cpuid() -> c_int {
    let id: u64;
    core::arch::asm!("mv {0}, tp", out(reg) id);
    id as c_int
}

#[no_mangle]
pub unsafe extern "C" fn mycpu() -> *mut Cpu {
    let id = cpuid() as usize;
    ptr::addr_of_mut!(cpus).cast::<Cpu>().add(id)
}

#[no_mangle]
pub unsafe extern "C" fn myproc() -> *mut Proc {
    let _irq = InterruptGuard::new();
    let c = mycpu();
    let p = (*c).proc.cast::<Proc>();
    p
}

#[no_mangle]
pub unsafe extern "C" fn myproc_sz() -> u64 {
    let p = myproc();
    if p.is_null() { 0 } else { (*p).sz }
}

#[no_mangle]
pub unsafe extern "C" fn myproc_pagetable() -> *mut u64 {
    let p = myproc();
    if p.is_null() { ptr::null_mut() } else { (*p).pagetable }
}

#[no_mangle]
pub unsafe extern "C" fn myproc_cwd() -> *mut c_void {
    let p = myproc();
    if p.is_null() { ptr::null_mut() } else { (*p).cwd }
}

#[no_mangle]
pub unsafe extern "C" fn setkilled(p: *mut Proc) {
    acquire(ptr::addr_of_mut!((*p).lock));
    (*p).killed = 1;
    release(ptr::addr_of_mut!((*p).lock));
}

#[no_mangle]
pub unsafe extern "C" fn killed(p: *mut Proc) -> c_int {
    acquire(ptr::addr_of_mut!((*p).lock));
    let k = (*p).killed;
    release(ptr::addr_of_mut!((*p).lock));
    k
}

/// Process ID allocator — demonstrates the safe `SpinMutex<T>` API.
///
/// The `unsafe` is fully localized inside `SpinMutex::lock()`; from the
/// caller's perspective, this is normal Rust code with no raw pointer
/// dereferences.
static PID_COUNTER: crate::rust_lock::SpinMutex<c_int> =
    crate::rust_lock::SpinMutex::new(1, b"pid_counter\0");

#[no_mangle]
pub extern "C" fn allocpid() -> c_int {
    let mut g = PID_COUNTER.lock();
    let pid = *g;
    *g += 1;
    pid
}

#[no_mangle]
pub unsafe extern "C" fn either_copyout(
    user_dst: c_int,
    dst: u64,
    src: *mut c_void,
    len: u64,
) -> c_int {
    let p = myproc();
    if user_dst != 0 {
        return copyout((*p).pagetable, dst, src.cast::<c_char>(), len);
    }
    memmove(dst as usize as *mut c_void, src as *const c_void, len as c_uint);
    0
}

#[no_mangle]
pub unsafe extern "C" fn either_copyin(
    dst: *mut c_void,
    user_src: c_int,
    src: u64,
    len: u64,
) -> c_int {
    let p = myproc();
    if user_src != 0 {
        return copyin((*p).pagetable, dst.cast::<c_char>(), src, len);
    }
    memmove(dst, src as usize as *const c_void, len as c_uint);
    0
}

#[no_mangle]
pub unsafe extern "C" fn procinit() {
    initlock(ptr::addr_of_mut!(pid_lock), b"nextpid\0".as_ptr() as *mut c_char);
    initlock(ptr::addr_of_mut!(wait_lock), b"wait_lock\0".as_ptr() as *mut c_char);

    let mut i = 0usize;
    let base = ptr::addr_of_mut!(proc).cast::<Proc>();
    while i < NPROC {
        let p = base.add(i);
        initlock(ptr::addr_of_mut!((*p).lock), b"proc\0".as_ptr() as *mut c_char);
        (*p).state = UNUSED;
        (*p).kstack = TRAMPOLINE - ((i as u64 + 1) * 2 * PGSIZE);
        i += 1;
    }
}

#[no_mangle]
pub unsafe extern "C" fn userinit() {
    let p = allocproc();
    initproc = p;

    (*p).cwd = namei(b"/\0".as_ptr() as *mut c_char).cast::<c_void>();
    (*p).state = RUNNABLE;

    release(ptr::addr_of_mut!((*p).lock));
}

#[no_mangle]
pub unsafe extern "C" fn growproc(n: c_int) -> c_int {
    let p = myproc();
    let mut sz = (*p).sz;

    if n > 0 {
        let delta = n as u64;
        if sz + delta > TRAPFRAME {
            return -1;
        }
        sz = uvmalloc((*p).pagetable, sz, sz + delta, PTE_W);
        if sz == 0 {
            return -1;
        }
    } else if n < 0 {
        let newsz = (sz as i64).wrapping_add(n as i64) as u64;
        sz = uvmdealloc((*p).pagetable, sz, newsz);
    }

    (*p).sz = sz;
    0
}

#[no_mangle]
pub unsafe extern "C" fn sys_exit() -> u64 {
    let mut n: c_int = 0;
    argint(0, ptr::addr_of_mut!(n));
    kexit(n);
}

#[no_mangle]
pub unsafe extern "C" fn sys_getpid() -> u64 {
    (*myproc()).pid as u64
}

#[no_mangle]
pub unsafe extern "C" fn sys_fork() -> u64 {
    kfork() as u64
}

#[no_mangle]
pub unsafe extern "C" fn sys_wait() -> u64 {
    let mut p: u64 = 0;
    argaddr(0, ptr::addr_of_mut!(p));
    kwait(p) as u64
}

#[no_mangle]
pub unsafe extern "C" fn sys_sbrk() -> u64 {
    let mut n: c_int = 0;
    let mut t: c_int = 0;
    argint(0, ptr::addr_of_mut!(n));
    argint(1, ptr::addr_of_mut!(t));

    let p = myproc();
    let addr = (*p).sz;

    if t == SBRK_EAGER || n < 0 {
        if growproc(n) < 0 {
            return u64::MAX;
        }
    } else {
        let delta = n as u64;
        let Some(newsz) = addr.checked_add(delta) else {
            return u64::MAX;
        };
        if newsz > TRAPFRAME {
            return u64::MAX;
        }
        (*p).sz = newsz;
    }

    addr
}

#[no_mangle]
pub unsafe extern "C" fn sys_pause() -> u64 {
    let mut n: c_int = 0;
    argint(0, ptr::addr_of_mut!(n));
    if n < 0 {
        n = 0;
    }

    acquire(ptr::addr_of_mut!(crate::rust_trap::tickslock));
    let ticks0 = crate::rust_trap::ticks;
    while crate::rust_trap::ticks.wrapping_sub(ticks0) < n as c_uint {
        if killed(myproc()) != 0 {
            release(ptr::addr_of_mut!(crate::rust_trap::tickslock));
            return u64::MAX;
        }
        sleep(
            ptr::addr_of_mut!(crate::rust_trap::ticks).cast::<c_void>(),
            ptr::addr_of_mut!(crate::rust_trap::tickslock),
        );
    }
    release(ptr::addr_of_mut!(crate::rust_trap::tickslock));
    0
}

#[no_mangle]
pub unsafe extern "C" fn sys_kill() -> u64 {
    let mut pid: c_int = 0;
    argint(0, ptr::addr_of_mut!(pid));
    kkill(pid) as u64
}

#[no_mangle]
pub unsafe extern "C" fn sys_uptime() -> u64 {
    acquire(ptr::addr_of_mut!(crate::rust_trap::tickslock));
    let xticks = crate::rust_trap::ticks;
    release(ptr::addr_of_mut!(crate::rust_trap::tickslock));
    xticks as u64
}

#[no_mangle]
pub unsafe extern "C" fn kkill(pid: c_int) -> c_int {
    let mut i = 0usize;
    let base = ptr::addr_of_mut!(proc).cast::<Proc>();

    while i < NPROC {
        let p = base.add(i);
        acquire(ptr::addr_of_mut!((*p).lock));
        if (*p).pid == pid {
            (*p).killed = 1;
            if (*p).state == SLEEPING {
                (*p).state = RUNNABLE;
            }
            release(ptr::addr_of_mut!((*p).lock));
            return 0;
        }
        release(ptr::addr_of_mut!((*p).lock));
        i += 1;
    }

    -1
}

#[no_mangle]
pub unsafe extern "C" fn r#yield() {
    let p = myproc();
    acquire(ptr::addr_of_mut!((*p).lock));
    (*p).state = RUNNABLE;
    sched();
    release(ptr::addr_of_mut!((*p).lock));
}

#[no_mangle]
pub unsafe extern "C" fn sleep(chan: *mut c_void, lk: *mut Spinlock) {
    let p = myproc();

    acquire(ptr::addr_of_mut!((*p).lock));
    release(lk);

    (*p).chan = chan;
    (*p).state = SLEEPING;

    sched();

    (*p).chan = ptr::null_mut();
    release(ptr::addr_of_mut!((*p).lock));
    acquire(lk);
}

#[no_mangle]
pub unsafe extern "C" fn wakeup(chan: *mut c_void) {
    let current = myproc();
    let mut i = 0usize;
    let base = ptr::addr_of_mut!(proc).cast::<Proc>();

    while i < NPROC {
        let p = base.add(i);
        if p != current {
            acquire(ptr::addr_of_mut!((*p).lock));
            if (*p).state == SLEEPING && (*p).chan == chan {
                (*p).state = RUNNABLE;
            }
            release(ptr::addr_of_mut!((*p).lock));
        }
        i += 1;
    }
}

#[no_mangle]
pub unsafe extern "C" fn scheduler() -> ! {
    let c = mycpu();
    (*c).proc = ptr::null_mut();

    loop {
        intr_on();
        intr_off();

        let mut found = 0;
        let mut i = 0usize;
        let base = ptr::addr_of_mut!(proc).cast::<Proc>();

        while i < NPROC {
            let p = base.add(i);
            acquire(ptr::addr_of_mut!((*p).lock));
            if (*p).state == RUNNABLE {
                (*p).state = RUNNING;
                (*c).proc = p.cast::<c_void>();
                swtch(
                    ptr::addr_of_mut!((*c).context),
                    ptr::addr_of_mut!((*p).context),
                );
                (*c).proc = ptr::null_mut();
                found = 1;
            }
            release(ptr::addr_of_mut!((*p).lock));
            i += 1;
        }

        if found == 0 {
            core::arch::asm!("wfi");
        }
    }
}

unsafe fn freeproc_locked(p: *mut Proc) {
    if !(*p).trapframe.is_null() {
        kfree((*p).trapframe.cast::<c_void>());
    }
    (*p).trapframe = ptr::null_mut();
    if !(*p).pagetable.is_null() {
        proc_freepagetable((*p).pagetable, (*p).sz);
    }
    (*p).pagetable = ptr::null_mut();
    (*p).sz = 0;
    (*p).pid = 0;
    (*p).parent = ptr::null_mut();
    (*p).name[0] = 0;
    (*p).chan = ptr::null_mut();
    (*p).killed = 0;
    (*p).xstate = 0;
    (*p).state = UNUSED;
}

unsafe fn reparent_locked(p: *mut Proc) {
    let mut i = 0usize;
    let base = ptr::addr_of_mut!(proc).cast::<Proc>();

    while i < NPROC {
        let pp = base.add(i);
        if (*pp).parent == p {
            (*pp).parent = initproc;
            wakeup(initproc.cast::<c_void>());
        }
        i += 1;
    }
}

#[no_mangle]
pub unsafe extern "C" fn kexit(status: c_int) -> ! {
    let p = myproc();

    if p == initproc {
        panic(b"init exiting\0".as_ptr() as *mut c_char);
    }

    let mut fd = 0usize;
    while fd < NOFILE {
        let f = (*p).ofile[fd];
        if !f.is_null() {
            fileclose(f.cast::<File>());
            (*p).ofile[fd] = ptr::null_mut();
        }
        fd += 1;
    }

    // IMPORTANT: kexit() never returns (ends in sched -> !), so a guard
    // kept alive to function end would never run Drop. Keep the log
    // transaction in a short scope so end_op() is guaranteed here.
    {
        let _tx = TxnGuard::begin();
        iput((*p).cwd.cast::<Inode>());
    }
    (*p).cwd = ptr::null_mut();

    acquire(ptr::addr_of_mut!(wait_lock));
    reparent_locked(p);
    wakeup((*p).parent.cast::<c_void>());

    acquire(ptr::addr_of_mut!((*p).lock));
    (*p).xstate = status;
    (*p).state = ZOMBIE;

    release(ptr::addr_of_mut!(wait_lock));

    sched();
    panic(b"zombie exit\0".as_ptr() as *mut c_char);
}

#[no_mangle]
pub unsafe extern "C" fn kwait(addr: u64) -> c_int {
    let p = myproc();
    let base = ptr::addr_of_mut!(proc).cast::<Proc>();

    acquire(ptr::addr_of_mut!(wait_lock));

    loop {
        let mut havekids = 0;
        let mut i = 0usize;

        while i < NPROC {
            let pp = base.add(i);
            if (*pp).parent == p {
                acquire(ptr::addr_of_mut!((*pp).lock));
                havekids = 1;
                if (*pp).state == ZOMBIE {
                    let pid = (*pp).pid;
                    if addr != 0
                        && copyout(
                            (*p).pagetable,
                            addr,
                            ptr::addr_of_mut!((*pp).xstate).cast::<c_char>(),
                            core::mem::size_of::<c_int>() as u64,
                        ) < 0
                    {
                        release(ptr::addr_of_mut!((*pp).lock));
                        release(ptr::addr_of_mut!(wait_lock));
                        return -1;
                    }
                    freeproc_locked(pp);
                    release(ptr::addr_of_mut!((*pp).lock));
                    release(ptr::addr_of_mut!(wait_lock));
                    return pid;
                }
                release(ptr::addr_of_mut!((*pp).lock));
            }
            i += 1;
        }

        if havekids == 0 || killed(p) != 0 {
            release(ptr::addr_of_mut!(wait_lock));
            return -1;
        }

        sleep(p.cast::<c_void>(), ptr::addr_of_mut!(wait_lock));
    }
}

#[no_mangle]
pub unsafe extern "C" fn sched() {
    let p = myproc();

    if holding(ptr::addr_of_mut!((*p).lock)) == 0 {
        panic(b"sched p->lock\0".as_ptr() as *mut c_char);
    }
    if (*mycpu()).noff != 1 {
        panic(b"sched locks\0".as_ptr() as *mut c_char);
    }
    if (*p).state == RUNNING {
        panic(b"sched RUNNING\0".as_ptr() as *mut c_char);
    }

    let c = mycpu();
    let intena = (*c).intena;
    swtch(ptr::addr_of_mut!((*p).context), ptr::addr_of_mut!((*c).context));
    (*c).intena = intena;
}

#[no_mangle]
pub unsafe extern "C" fn forkret() -> ! {
    let p = myproc();

    release(ptr::addr_of_mut!((*p).lock));

    if FORKRET_FIRST != 0 {
        fsinit(ROOTDEV);

        FORKRET_FIRST = 0;
        fence(Ordering::SeqCst);

        let path = b"/init\0".as_ptr() as *mut c_char;
        let mut argv = [path, ptr::null_mut()];
        (*(*p).trapframe).a0 = kexec(path, argv.as_mut_ptr()) as u64;
        if (*(*p).trapframe).a0 == u64::MAX {
            panic(b"exec\0".as_ptr() as *mut c_char);
        }
    }

    prepare_return();
    let satp = make_satp((*p).pagetable);
    let trampoline_userret = TRAMPOLINE
        + ((ptr::addr_of!(userret) as u64).wrapping_sub(ptr::addr_of!(trampoline) as u64));
    let trampoline_fn: extern "C" fn(u64) -> ! = core::mem::transmute(trampoline_userret as usize);
    trampoline_fn(satp);
}

#[no_mangle]
pub unsafe extern "C" fn procdump() {
    dump_putc(b'\n');

    let mut i = 0usize;
    let base = ptr::addr_of_mut!(proc).cast::<Proc>();

    while i < NPROC {
        let p = base.add(i);
        let state = (*p).state;
        if state != UNUSED {
            rust_printint((*p).pid as i64, 10, 1);
            dump_putc(b' ');

            let state_name: &[u8] = match state {
                0 => b"unused",
                1 => b"used",
                2 => b"sleep ",
                3 => b"runble",
                4 => b"run   ",
                5 => b"zombie",
                _ => b"???",
            };
            dump_bytes(state_name);
            dump_putc(b' ');

            dump_name(&(*p).name);
            dump_putc(b'\n');
        }
        i += 1;
    }
}
