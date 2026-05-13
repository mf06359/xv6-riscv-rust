//! Cargo entry point for the `echo` user binary. This wrapper just
//! re-roots the existing source files (`../echo.rs` plus the four
//! runtime helpers) into a single crate so Cargo can drive the build.

#![no_std]
#![allow(dead_code, unused_attributes)]

#[path = "../ulib.rs"] pub mod ulib;
#[path = "../usys.rs"] pub mod usys;
#[path = "../printf.rs"] pub mod printf;
#[path = "../umalloc.rs"] pub mod umalloc;
#[path = "../echo.rs"] pub mod prog;
