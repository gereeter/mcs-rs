#![feature(asm, const_fn, generic_param_attrs, dropck_eyepatch)]

#![no_std]

mod mutex;
mod pause;

pub use mutex::{Slot, Mutex, Guard};
