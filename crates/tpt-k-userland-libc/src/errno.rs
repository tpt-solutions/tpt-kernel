//! POSIX errno values, with the numbers Linux and most Unix systems use.

/// An error number. The discriminant is the value a C caller sees.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Errno {
    /// Operation not permitted.
    EPERM = 1,
    /// No such file or directory.
    ENOENT = 2,
    /// Input or output error.
    EIO = 5,
    /// Bad file descriptor.
    EBADF = 9,
    /// Out of memory.
    ENOMEM = 12,
    /// Permission denied.
    EACCES = 13,
    /// Bad address.
    EFAULT = 14,
    /// Invalid argument.
    EINVAL = 22,
    /// No space left on device.
    ENOSPC = 28,
    /// Function not implemented.
    ENOSYS = 38,
}

impl Errno {
    /// The numeric value of the error.
    #[must_use]
    pub const fn code(self) -> i32 {
        self as i32
    }

    /// The error with numeric value `code`, or `None` if it is not one of the
    /// values this crate defines.
    #[must_use]
    pub const fn from_code(code: i32) -> Option<Self> {
        Some(match code {
            1 => Self::EPERM,
            2 => Self::ENOENT,
            5 => Self::EIO,
            9 => Self::EBADF,
            12 => Self::ENOMEM,
            13 => Self::EACCES,
            14 => Self::EFAULT,
            22 => Self::EINVAL,
            28 => Self::ENOSPC,
            38 => Self::ENOSYS,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_match_the_unix_numbers() {
        assert_eq!(Errno::EPERM.code(), 1);
        assert_eq!(Errno::ENOENT.code(), 2);
        assert_eq!(Errno::EINVAL.code(), 22);
        assert_eq!(Errno::ENOSYS.code(), 38);
    }

    #[test]
    fn round_trips_through_codes() {
        for e in [
            Errno::EPERM,
            Errno::ENOENT,
            Errno::EIO,
            Errno::EBADF,
            Errno::ENOMEM,
            Errno::EACCES,
            Errno::EFAULT,
            Errno::EINVAL,
            Errno::ENOSPC,
            Errno::ENOSYS,
        ] {
            assert_eq!(Errno::from_code(e.code()), Some(e));
        }
        assert_eq!(Errno::from_code(0), None);
        assert_eq!(Errno::from_code(999), None);
    }
}
