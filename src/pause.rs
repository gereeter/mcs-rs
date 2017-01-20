/// Do something to wait in spinlocks and use less CPU
#[inline(always)]

#[cfg(all(feature = "unstable", any(target_arch = "x86", target_arch = "x86_64")))]
pub fn pause() {
    unsafe { asm!("pause" :::: "volatile"); }
}

#[cfg(any(not(feature = "unstable"), not(any(target_arch = "x86", target_arch = "x86_64"))))]
pub fn pause() { }

