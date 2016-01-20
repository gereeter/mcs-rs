/// Do something to wait in spinlocks and use less CPU
#[inline(always)]

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub fn pause() {
    unsafe { asm!("pause" :::: "volatile"); }
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub fn pause() { }

