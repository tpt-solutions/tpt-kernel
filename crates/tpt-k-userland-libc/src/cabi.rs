//! C-ABI exports for the string and memory helpers.
//!
//! Built only with the `c-abi` feature. The symbols use the C names, so a
//! program that links them must not also link another libc that defines the
//! same names. Each function checks nothing beyond what C requires of its
//! caller, and does the same work as the slice function it wraps.

use core::slice;

use crate::string;

/// Length of the NUL-terminated string at `s`, not counting the NUL.
///
/// # Safety
/// `s` must point to a readable NUL-terminated byte string.
#[no_mangle]
pub unsafe extern "C" fn strlen(s: *const u8) -> usize {
    let mut n = 0;
    while *s.add(n) != 0 {
        n += 1;
    }
    n
}

/// Length of the string at `s`, looking at no more than `max` bytes.
///
/// # Safety
/// `s` must be readable for `max` bytes, or up to its first NUL if that comes
/// first.
#[no_mangle]
pub unsafe extern "C" fn strnlen(s: *const u8, max: usize) -> usize {
    let mut n = 0;
    while n < max && *s.add(n) != 0 {
        n += 1;
    }
    n
}

/// Compares two NUL-terminated strings, as C's `strcmp` does.
///
/// # Safety
/// `a` and `b` must each point to a readable NUL-terminated byte string.
#[no_mangle]
pub unsafe extern "C" fn strcmp(a: *const u8, b: *const u8) -> i32 {
    let la = strlen(a) + 1;
    let lb = strlen(b) + 1;
    string::strcmp(slice::from_raw_parts(a, la), slice::from_raw_parts(b, lb))
}

/// Compares up to `n` bytes of two strings, as C's `strncmp` does.
///
/// # Safety
/// `a` and `b` must each be readable for `n` bytes, or up to and including
/// their NUL terminators if those come first.
#[no_mangle]
pub unsafe extern "C" fn strncmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    let la = strnlen(a, n).min(n);
    let lb = strnlen(b, n).min(n);
    // Include the terminator when it is within the first `n` bytes.
    let la = if la < n { la + 1 } else { la };
    let lb = if lb < n { lb + 1 } else { lb };
    string::strncmp(
        slice::from_raw_parts(a, la),
        slice::from_raw_parts(b, lb),
        n,
    )
}

/// Copies `n` bytes from `src` to `dst`, as C's `memcpy` does. Returns `dst`.
///
/// The loops use volatile accesses on purpose. A plain copy or fill would be
/// lowered by LLVM to a call to `memcpy` or `memset`, which is this function,
/// and the call would never return.
///
/// # Safety
/// `dst` must be writable for `n` bytes, `src` readable for `n` bytes, and the
/// two ranges must not overlap.
#[no_mangle]
pub unsafe extern "C" fn memcpy(dst: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    for i in 0..n {
        core::ptr::write_volatile(dst.add(i), core::ptr::read_volatile(src.add(i)));
    }
    dst
}

/// Sets `n` bytes at `dst` to `value`, as C's `memset` does. Returns `dst`.
///
/// Uses volatile writes for the same reason as [`memcpy`].
///
/// # Safety
/// `dst` must be writable for `n` bytes.
#[no_mangle]
pub unsafe extern "C" fn memset(dst: *mut u8, value: i32, n: usize) -> *mut u8 {
    for i in 0..n {
        core::ptr::write_volatile(dst.add(i), value as u8);
    }
    dst
}

/// Compares `n` bytes of two buffers, as C's `memcmp` does.
///
/// # Safety
/// `a` and `b` must each be readable for `n` bytes.
#[no_mangle]
pub unsafe extern "C" fn memcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    for i in 0..n {
        let x = core::ptr::read_volatile(a.add(i));
        let y = core::ptr::read_volatile(b.add(i));
        if x != y {
            return i32::from(x) - i32::from(y);
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strlen_stops_at_nul() {
        let s = b"hello\0junk\0";
        assert_eq!(unsafe { strlen(s.as_ptr()) }, 5);
        assert_eq!(unsafe { strnlen(s.as_ptr(), 3) }, 3);
        assert_eq!(unsafe { strnlen(s.as_ptr(), 100) }, 5);
    }

    #[test]
    fn strcmp_and_strncmp_follow_c() {
        let a = b"apple\0";
        let b = b"apply\0";
        assert!(unsafe { strcmp(a.as_ptr(), b.as_ptr()) } < 0);
        assert_eq!(unsafe { strcmp(a.as_ptr(), a.as_ptr()) }, 0);
        assert_eq!(unsafe { strncmp(a.as_ptr(), b.as_ptr(), 4) }, 0);
        assert!(unsafe { strncmp(a.as_ptr(), b.as_ptr(), 5) } < 0);
        // A string shorter than `n` compares equal to its own prefix only.
        let short = b"app\0";
        assert!(unsafe { strncmp(short.as_ptr(), a.as_ptr(), 10) } < 0);
    }

    #[test]
    fn memcpy_memset_memcmp_work_on_buffers() {
        let src = [1u8, 2, 3, 4];
        let mut dst = [0u8; 4];
        let r = unsafe { memcpy(dst.as_mut_ptr(), src.as_ptr(), 4) };
        assert_eq!(r, dst.as_mut_ptr());
        assert_eq!(dst, src);
        unsafe { memset(dst.as_mut_ptr(), 9, 2) };
        assert_eq!(dst, [9, 9, 3, 4]);
        assert_eq!(unsafe { memcmp(dst.as_ptr(), src.as_ptr(), 0) }, 0);
        assert!(unsafe { memcmp(dst.as_ptr(), src.as_ptr(), 1) } > 0);
    }
}
