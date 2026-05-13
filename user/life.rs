// Conway's Game of Life — animated on the console.
//
// usage:  life [generations]    (default 200)
//
// The grid wraps around (toroidal topology). A few classic patterns
// (glider, blinker, beacon, R-pentomino) are seeded at startup.
// ANSI escape sequences are used to home the cursor each frame so the
// animation stays in place rather than scrolling.

#![no_std]
#![allow(dead_code)]

use core::ffi::{c_char, c_int, c_void};

mod rust_user;
use rust_user::*;

const W: usize = 50;
const H: usize = 18;
const N: usize = W * H;

#[inline(always)]
fn idx(x: usize, y: usize) -> usize {
    y * W + x
}

#[inline(always)]
unsafe fn cell(g: &[u8; N], x: usize, y: usize) -> u8 {
    *g.get_unchecked(idx(x, y))
}

#[inline(always)]
unsafe fn cell_mut(g: &mut [u8; N], x: usize, y: usize, v: u8) {
    *g.get_unchecked_mut(idx(x, y)) = v;
}

unsafe fn neighbors(g: &[u8; N], x: usize, y: usize) -> u8 {
    let mut n: u8 = 0;
    for dy in -1i32..=1 {
        for dx in -1i32..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let xx = ((x as i32 + dx + W as i32) % W as i32) as usize;
            let yy = ((y as i32 + dy + H as i32) % H as i32) as usize;
            n += cell(g, xx, yy);
        }
    }
    n
}

unsafe fn step(curr: &[u8; N], next: &mut [u8; N]) {
    for y in 0..H {
        for x in 0..W {
            let n = neighbors(curr, x, y);
            let alive = cell(curr, x, y) != 0;
            let live_next = (alive && (n == 2 || n == 3)) || (!alive && n == 3);
            cell_mut(next, x, y, live_next as u8);
        }
    }
}

// Frame buffer: one full screenful built in user memory and emitted with
// a single `write` syscall. Each frame is bounded by the static layout
// below — header (~80) + top border (~58) + 18 rows (~58 each) + bottom
// border + footer + a tiny safety margin.
const FRAME_CAP: usize = 2048;
struct Frame {
    buf: [u8; FRAME_CAP],
    len: usize,
}

#[inline(always)]
unsafe fn fput(f: &mut Frame, s: &[u8]) {
    let mut i = 0;
    let n = s.len();
    while i < n {
        *f.buf.get_unchecked_mut(f.len + i) = *s.get_unchecked(i);
        i += 1;
    }
    f.len += n;
}

#[inline(always)]
unsafe fn fput_byte(f: &mut Frame, c: u8) {
    *f.buf.get_unchecked_mut(f.len) = c;
    f.len += 1;
}

unsafe fn fput_int(f: &mut Frame, mut x: c_int) {
    let mut tmp = [0u8; 12];
    let mut i = 0usize;
    let neg = x < 0;
    if neg { x = -x; }
    if x == 0 {
        fput_byte(f, b'0');
        return;
    }
    while x > 0 {
        *tmp.get_unchecked_mut(i) = b'0' + (x % 10) as u8;
        i += 1;
        x /= 10;
    }
    if neg {
        fput_byte(f, b'-');
    }
    while i > 0 {
        i -= 1;
        fput_byte(f, *tmp.get_unchecked(i));
    }
}

unsafe fn count_alive(g: &[u8; N]) -> c_int {
    let mut c: c_int = 0;
    for i in 0..N {
        c += *g.get_unchecked(i) as c_int;
    }
    c
}

unsafe fn render(g: &[u8; N], gen: c_int) {
    let mut frame = Frame { buf: [0; FRAME_CAP], len: 0 };
    let f = &mut frame;

    // Cursor home (don't clear, less flicker).
    fput(f, b"\x1b[H");
    fput(f, b"\x1b[1;36m  Conway's Game of Life\x1b[0m   gen=\x1b[33m");
    fput_int(f, gen);
    fput(f, b"\x1b[0m  alive=\x1b[33m");
    fput_int(f, count_alive(g));
    fput(f, b"\x1b[0m   \n\n");

    // Top border
    fput(f, b"  +");
    for _ in 0..W { fput_byte(f, b'-'); }
    fput(f, b"+\n");

    for y in 0..H {
        fput(f, b"  |");
        for x in 0..W {
            fput_byte(f, if cell(g, x, y) != 0 { b'#' } else { b' ' });
        }
        fput(f, b"|\n");
    }

    // Bottom border
    fput(f, b"  +");
    for _ in 0..W { fput_byte(f, b'-'); }
    fput(f, b"+\n\n");
    fput(f, b"  (Ctrl-P: procdump)\n");

    // Single syscall for the whole frame.
    write(1, f.buf.as_ptr().cast::<c_void>(), f.len as c_int);
}

unsafe fn place(g: &mut [u8; N], ox: usize, oy: usize, pat: &[(usize, usize)]) {
    for &(dx, dy) in pat.iter() {
        let x = (ox + dx) % W;
        let y = (oy + dy) % H;
        cell_mut(g, x, y, 1);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn main(argc: c_int, argv: *mut *mut c_char) -> c_int {
    let gens: c_int = if argc > 1 { atoi(*argv.add(1)) } else { 200 };

    let mut grid: [u8; N] = [0; N];
    let mut next: [u8; N] = [0; N];

    // Glider — moves to the south-east.
    place(&mut grid, 2, 1, &[(1, 0), (2, 1), (0, 2), (1, 2), (2, 2)]);
    // Blinker — period 2.
    place(&mut grid, 22, 4, &[(0, 0), (1, 0), (2, 0)]);
    // Beacon — period 2.
    place(&mut grid, 12, 10, &[(0, 0), (1, 0), (0, 1), (3, 2), (2, 3), (3, 3)]);
    // R-pentomino — chaotic small pattern.
    place(&mut grid, 35, 8, &[(1, 0), (2, 0), (0, 1), (1, 1), (1, 2)]);
    // Toad — period 2.
    place(&mut grid, 30, 14, &[(1, 0), (2, 0), (3, 0), (0, 1), (1, 1), (2, 1)]);

    let clr = b"\x1b[2J";
    write(1, clr.as_ptr().cast::<c_void>(), clr.len() as c_int);

    for g in 0..gens {
        render(&grid, g);
        step(&grid, &mut next);
        core::mem::swap(&mut grid, &mut next);
        pause(2); // ~200ms per frame (2 timer ticks)
    }

    let nl = b"\n";
    write(1, nl.as_ptr().cast::<c_void>(), 1);
    0
}
