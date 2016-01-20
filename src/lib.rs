#![feature(asm, const_fn, dropck_parametricity)]

#![no_std]

use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

pub struct Slot {
    next: AtomicPtr<AtomicBool>
}

pub struct Guard<'a, T: ?Sized + 'a> {
    lock: &'a Mutex<T>,
    slot: &'a Slot
}

pub struct Mutex<T: ?Sized> {
    queue: AtomicPtr<Slot>,
    data: UnsafeCell<T>
}

unsafe impl<T: Send> Sync for Mutex<T> { }
unsafe impl<T: Send> Send for Mutex<T> { }


/// Do something to wait in spinlocks and use less CPU
#[inline(always)]

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn pause() {
    unsafe { asm!("pause" :::: "volatile"); }
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
fn pause() { }


impl Slot {
    pub const fn new() -> Slot {
        Slot {
            next: AtomicPtr::new(ptr::null_mut())
        }
    }
}

impl<T> Mutex<T> {
    pub const fn new(value: T) -> Mutex<T> {
        Mutex {
            queue: AtomicPtr::new(ptr::null_mut()),
            data: UnsafeCell::new(value)
        }
    }

    pub fn into_inner(self) -> T {
        unsafe {
            self.data.into_inner()
        }
    }
}

impl<T: ?Sized> Mutex<T> {
    pub fn try_lock<'a>(&'a self, slot: &'a mut Slot) -> Option<Guard<'a, T>> {
        slot.next = AtomicPtr::new(ptr::null_mut());

        if self.queue.compare_and_swap(ptr::null_mut(), slot, Ordering::SeqCst).is_null() {
            Some(Guard {
                lock: self,
                slot: slot
            })
        } else {
            None
        }
    }

    pub fn lock<'a>(&'a self, slot: &'a mut Slot) -> Guard<'a, T> {
        slot.next = AtomicPtr::new(ptr::null_mut());
        let pred = self.queue.swap(slot, Ordering::SeqCst);
        if !pred.is_null() {
            let pred = unsafe { &*pred };
            let locked = AtomicBool::new(true);
            pred.next.store(&locked as *const _ as *mut _, Ordering::SeqCst);
            while locked.load(Ordering::SeqCst) {
                pause();
            }
        }

        Guard {
            lock: self,
            slot: slot
        }
    }

    pub fn get_mut(&mut self) -> &mut T {
        unsafe { &mut *self.data.get() }
    }
}

impl<'a, T: ?Sized> Deref for Guard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}

impl<'a, T: ?Sized> DerefMut for Guard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<'a, T: ?Sized> Drop for Guard<'a, T> {
    #[unsafe_destructor_blind_to_params]
    fn drop(&mut self) {
        let succ = self.slot.next.load(Ordering::SeqCst);
        if succ.is_null() {
            let slot_ptr = self.slot as *const _ as *mut _;
            if self.lock.queue.compare_and_swap(slot_ptr, ptr::null_mut(), Ordering::SeqCst) != slot_ptr {
                let mut succ;
                loop {
                    succ = self.slot.next.load(Ordering::SeqCst);
                    if !succ.is_null() {
                        break;
                    }
                    pause();
                }
                let succ = unsafe { &*succ };
                succ.store(false, Ordering::SeqCst);
            }
        } else {
            let succ = unsafe { &*succ };
            succ.store(false, Ordering::SeqCst);
        }
    }
}

#[cfg(test)]
mod test {
    extern crate std;

    use super::{Mutex, Slot};

    // Mostly stoled from the Rust standard Mutex implementation's tests, so

    // Copyright 2014 The Rust Project Developers. See the COPYRIGHT
    // file at http://rust-lang.org/COPYRIGHT.
    //
    // Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
    // http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
    // <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
    // option. This file may not be copied, modified, or distributed
    // except according to those terms.

    use self::std::sync::Arc;
    use self::std::sync::mpsc::channel;
    use self::std::sync::atomic::{AtomicUsize, Ordering};
    use self::std::thread;

    #[derive(Eq, PartialEq, Debug)]
    struct NonCopy(i32);

    #[test]
    fn smoke() {
        let mut slot = Slot::new();
        let m = Mutex::new(());
        drop(m.lock(&mut slot));
        drop(m.lock(&mut slot));
    }

    #[test]
    fn lots_and_lots() {
        static LOCK: Mutex<u32> = Mutex::new(0);
        const ITERS: u32 = 1000;
        const CONCURRENCY: u32 = 3;

        fn inc() {
            let mut slot = Slot::new();
            for _ in 0..ITERS {
                let mut g = LOCK.lock(&mut slot);
                *g += 1;
            }
        };

        let (tx, rx) = channel();
        for _ in 0..CONCURRENCY {
            let tx2 = tx.clone();
            thread::spawn(move|| { inc(); tx2.send(()).unwrap(); });
            let tx2 = tx.clone();
            thread::spawn(move|| { inc(); tx2.send(()).unwrap(); });
        }

        drop(tx);
        for _ in 0..2 * CONCURRENCY {
            rx.recv().unwrap();
        }
        let mut slot = Slot::new();
        assert_eq!(*LOCK.lock(&mut slot), ITERS * CONCURRENCY * 2);
    }

    #[test]
    fn try_lock() {
        let mut slot = Slot::new();
        let m = Mutex::new(());
        *m.try_lock(&mut slot).unwrap() = ();
    }

    #[test]
    fn test_into_inner() {
        let m = Mutex::new(NonCopy(10));
        assert_eq!(m.into_inner(), NonCopy(10));
    }

    #[test]
    fn test_into_inner_drop() {
        struct Foo(Arc<AtomicUsize>);
        impl Drop for Foo {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let num_drops = Arc::new(AtomicUsize::new(0));
        let m = Mutex::new(Foo(num_drops.clone()));
        assert_eq!(num_drops.load(Ordering::SeqCst), 0);
        {
            let _inner = m.into_inner();
            assert_eq!(num_drops.load(Ordering::SeqCst), 0);
        }
        assert_eq!(num_drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_get_mut() {
        let mut m = Mutex::new(NonCopy(10));
        *m.get_mut() = NonCopy(20);
        assert_eq!(m.into_inner(), NonCopy(20));
    }

    #[test]
    fn test_lock_arc_nested() {
        // Tests nested locks and access
        // to underlying data.
        let arc = Arc::new(Mutex::new(1));
        let arc2 = Arc::new(Mutex::new(arc));
        let (tx, rx) = channel();
        let _t = thread::spawn(move|| {
            let mut slot1 = Slot::new();
            let mut slot2 = Slot::new();

            let lock = arc2.lock(&mut slot1);
            let lock2 = lock.lock(&mut slot2);
            assert_eq!(*lock2, 1);
            tx.send(()).unwrap();
        });
        rx.recv().unwrap();
    }

    #[test]
    fn test_lock_arc_access_in_unwind() {
        let arc = Arc::new(Mutex::new(1));
        let arc2 = arc.clone();
        let _ = thread::spawn(move|| -> () {
            struct Unwinder {
                i: Arc<Mutex<i32>>,
            }
            impl Drop for Unwinder {
                fn drop(&mut self) {
                    let mut slot = Slot::new();
                    *self.i.lock(&mut slot) += 1;
                }
            }
            let _u = Unwinder { i: arc2 };
            panic!();
        }).join();
        let mut slot = Slot::new();
        let lock = arc.lock(&mut slot);
        assert_eq!(*lock, 2);
    }

    #[test]
    fn test_lock_unsized() {
        let mut slot = Slot::new();
        let lock: &Mutex<[i32]> = &Mutex::new([1, 2, 3]);
        {
            let b = &mut *lock.lock(&mut slot);
            b[0] = 4;
            b[2] = 5;
        }
        let comp: &[i32] = &[4, 2, 5];
        assert_eq!(&*lock.lock(&mut slot), comp);
    }
}
