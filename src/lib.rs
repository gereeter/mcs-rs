#![feature(asm, const_fn, dropck_parametricity)]

#![no_std]

mod mutex;
mod pause;

pub use mutex::{Slot, Mutex, Guard};
