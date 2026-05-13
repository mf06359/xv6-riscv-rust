#![no_std]
#![allow(dead_code)]

use core::ffi::{c_char, c_int, c_void};

mod rust_user;
use rust_user::*;

const ESC_CLEAR: &'static str = "\x1b[2J";
const ESC_HOME: &'static str = "\x1b[H";
const ESC_RESET: &'static str = "\x1b[0m";
const ESC_SHOW_CURSOR: &'static str = "\x1b[?25h";

const SCREEN_WIDTH: isize = 52;
const TRAIN_LINES: usize = 44;
const SPACES: &'static str = "                                                                                ";

const STR_XX: &'static str = "                                                    ";
const STR_00: &'static str = "                       -+                           ";
const STR_01: &'static str = "                     *@=                            ";
const STR_02: &'static str = "                   -%-   -=                         ";
const STR_03: &'static str = "                  =%.  -@=                          ";
const STR_04: &'static str = "                  #:  -@   #:                       ";
const STR_05: &'static str = "                 :%   %#  @#     *##*:            ::";
const STR_06: &'static str = "                 -%   :.        @=  -#.           =*";
const STR_07: &'static str = "                  .          :+%*%==*#  .%+  =+   =+";
const STR_08: &'static str = "                            ##:   --.   =+  :#-  .*=";
const STR_09: &'static str = "                          :@-              =#-   +* ";
const STR_10: &'static str = "                          @:                    +*. ";
const STR_11: &'static str = "                     :-++@%                   :#+   ";
const STR_12: &'static str = "                :#%%=      :#%%.             :+.    ";
const STR_13: &'static str = "              *@=              *@-                  ";
const STR_14: &'static str = "            *%:                  *#                 ";
const STR_15: &'static str = "          =%-                     :%-               ";
const STR_16: &'static str = "         **                        :%=              ";
const STR_17: &'static str = "       .*+     .=           :       :%+             ";
const STR_18: &'static str = "      .#+      **          .@        :%-            ";
const STR_19: &'static str = "      *+                              -#.           ";
const STR_20: &'static str = "     +*.                               **           ";
const STR_21: &'static str = "    -#:                                 #:          ";
const STR_22: &'static str = "    *=            *#  /:                **          ";
const STR_23: &'static str = "   -#:             -**+                 :*          ";
const STR_24: &'static str = "   =+                                   :#:         ";
const STR_25: &'static str = "   +=                                  %@%-         ";
const STR_26: &'static str = "  +@=                                   :@@+        ";
const STR_27: &'static str = " **++                                   :#-=#:      ";
const STR_28: &'static str = "=* =*.                                  -*  -#-     ";
const STR_29: &'static str = ".. :#-                                 .#+          ";
const STR_30: &'static str = "    +*                                .=#           ";
const STR_31: &'static str = "    .*+.                             .:%-           ";
const STR_32: &'static str = "      **.                            =%-            ";
const STR_33: &'static str = "       -#*:.                     ..-#+      .       ";
const STR_34: &'static str = "         :*#%*-:.            .:=#%#+      -#=.+:    ";
const STR_35: &'static str = "               .-=**#######*+-.         .**. -= .*= ";
const STR_36: &'static str = "                                                    ";
const STR_37: &'static str = "                   .........                        ";
const STR_38: &'static str = "            ....::::::::::::::::....                ";
const STR_39: &'static str = "          ...::::-------------:::::...              ";
const STR_40: &'static str = "          ....:::::----------:::::...               ";
const STR_41: &'static str = "              .......::::::.......                  ";

const FRAME_A: [&'static str; TRAIN_LINES] = [
    STR_00, STR_01, STR_02, STR_03, STR_04, STR_05, STR_06, STR_07, STR_08, STR_09, STR_10,
    STR_11, STR_12, STR_13, STR_14, STR_15, STR_16, STR_17, STR_18, STR_19, STR_20, STR_21,
    STR_22, STR_23, STR_24, STR_25, STR_26, STR_27, STR_28, STR_29, STR_30, STR_31, STR_32,
    STR_33, STR_34, STR_35, STR_36, STR_37, STR_38, STR_39, STR_40, STR_41, STR_XX, STR_XX,
];

const FRAME_B: [&'static str; TRAIN_LINES] = [
    STR_XX, STR_XX, STR_00, STR_01, STR_02, STR_03, STR_04, STR_05, STR_06, STR_07, STR_08, 
    STR_09, STR_10, STR_11, STR_12, STR_13, STR_14, STR_15, STR_16, STR_17, STR_18, STR_19, 
    STR_20, STR_21, STR_22, STR_23, STR_24, STR_25, STR_26, STR_27, STR_28, STR_29, STR_30,
    STR_31, STR_32, STR_34, STR_34, STR_35, STR_36, STR_37, STR_38, STR_39, STR_40, STR_41,
];

#[inline(always)]
unsafe fn write_bytes(bytes: &[u8]) {
    let _ = write(1, bytes.as_ptr().cast::<c_void>(), bytes.len() as c_int);
}

#[inline(always)]
unsafe fn write_str(s: &'static str) {
    write_bytes(s.as_bytes());
}

unsafe fn write_spaces(mut n: usize) {
    while n > 0 {
        let chunk = if n > SPACES.len() { SPACES.len() } else { n };
        let _ = write(
            1,
            SPACES.as_bytes().as_ptr().cast::<c_void>(),
            chunk as c_int,
        );
        n -= chunk;
    }
}

fn frame_width(frame: &[&'static str; TRAIN_LINES]) -> usize {
    let mut i = 0;
    let mut max_len = 0;
    while i < TRAIN_LINES {
        let len = unsafe { frame.get_unchecked(i).len() };
        if len > max_len {
            max_len = len;
        }
        i += 1;
    }
    max_len
}

unsafe fn draw_line_clipped(x: isize, line: &'static str) {
    if x >= SCREEN_WIDTH || x + line.len() as isize <= 0 {
        write_str("\n");
        return;
    }

    let mut left_pad = 0usize;
    let mut start = 0usize;

    if x >= 0 {
        left_pad = x as usize;
    } else {
        start = (-x) as usize;
    }

    if left_pad >= SCREEN_WIDTH as usize || start >= line.len() {
        write_str("\n");
        return;
    }

    let avail = SCREEN_WIDTH as usize - left_pad;
    let remain = line.len() - start;
    let take = if remain > avail { avail } else { remain };

    write_spaces(left_pad);
    let _ = write(
        1,
        line.as_bytes().as_ptr().add(start).cast::<c_void>(),
        take as c_int,
    );
    write_str("\n");
}

unsafe fn draw_frame(x: isize, frame: &[&'static str; TRAIN_LINES]) {
    write_str(ESC_HOME);
    write_str(ESC_CLEAR);
    write_str(ESC_HOME);

    let mut i = 0;
    while i < TRAIN_LINES {
        draw_line_clipped(x, *frame.get_unchecked(i));
        i += 1;
    }
}

unsafe fn wait_ticks(delay: c_int) {
    if delay <= 0 {
        return;
    }
    let start = uptime();
    while uptime().wrapping_sub(start) < delay {}
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn main(argc: c_int, argv: *mut *mut c_char) -> c_int {
    let delay = if argc > 1 {
        let d = atoi(argv_at(argv, 1));
        if d > 0 { d } else { 1 }
    } else {
        1
    };

    let wa = frame_width(&FRAME_A);
    let wb = frame_width(&FRAME_B);
    let train_w = if wa > wb { wa } else { wb } as isize;

    // Avoid hiding the cursor permanently if the program exits unexpectedly.
    write_str(ESC_CLEAR);
    write_str(ESC_HOME);

    let mut x = SCREEN_WIDTH;
    let mut tick = 0;
    while x > -train_w {
        if (tick & (1 << 3)) == 0 {
            draw_frame(x, &FRAME_A);
        } else {
            draw_frame(x, &FRAME_B);
        }
        wait_ticks(delay);
        x -= 1;
        tick += 1;
    }

    write_str(ESC_CLEAR);
    write_str(ESC_HOME);
    write_str(ESC_RESET);
    write_str(ESC_SHOW_CURSOR);
    write_str("\n");

    0
}
