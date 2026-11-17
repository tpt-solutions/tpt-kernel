//! Path normalisation into a caller-supplied buffer.
//!
//! Paths are normalised against the root. The result always starts with `/`:
//! a relative input is read as relative to the root. Repeated slashes and `.`
//! components are dropped. `..` removes the component before it, and a `..`
//! that would climb above the root is refused.

/// Why a path could not be normalised.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathError {
    /// A `..` component climbs above the root.
    Escapes,
    /// The normalised path does not fit in the output buffer.
    Truncated,
    /// The path contains a NUL byte, which cannot appear in a file name.
    InvalidByte,
}

/// Normalises `input` into `out` and returns the length written.
///
/// Empty input and input made only of slashes and `.` components give `/`.
/// The output never ends in a slash except for the root itself.
pub fn normalize(input: &[u8], out: &mut [u8]) -> Result<usize, PathError> {
    let first = out.first_mut().ok_or(PathError::Truncated)?;
    *first = b'/';
    let mut len = 1;

    for component in input.split(|&b| b == b'/') {
        if component.contains(&0) {
            return Err(PathError::InvalidByte);
        }
        match component {
            b"" | b"." => {}
            b".." => {
                if len == 1 {
                    return Err(PathError::Escapes);
                }
                // Drop the last component: everything from its separator on.
                let sep = out[..len].iter().rposition(|&b| b == b'/').unwrap_or(0);
                len = if sep == 0 { 1 } else { sep };
            }
            name => {
                let needed = if len == 1 { name.len() } else { name.len() + 1 };
                let end = len.checked_add(needed).ok_or(PathError::Truncated)?;
                let dst = out.get_mut(len..end).ok_or(PathError::Truncated)?;
                let start = if len == 1 { 0 } else { 1 };
                dst[start..].copy_from_slice(name);
                if len != 1 {
                    dst[0] = b'/';
                }
                len = end;
            }
        }
    }
    Ok(len)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn norm(input: &[u8]) -> Result<std::string::String, PathError> {
        let mut out = [0u8; 64];
        let n = normalize(input, &mut out)?;
        Ok(std::string::String::from_utf8(out[..n].to_vec()).unwrap())
    }

    #[test]
    fn collapses_dots_and_slashes() {
        assert_eq!(norm(b"/a/./b/../c").unwrap(), "/a/c");
        assert_eq!(norm(b"//a///b/").unwrap(), "/a/b");
        assert_eq!(norm(b"/a/b/.").unwrap(), "/a/b");
        assert_eq!(norm(b"a/b").unwrap(), "/a/b");
    }

    #[test]
    fn root_and_empty_inputs() {
        assert_eq!(norm(b"").unwrap(), "/");
        assert_eq!(norm(b"/").unwrap(), "/");
        assert_eq!(norm(b"///").unwrap(), "/");
        assert_eq!(norm(b"/.").unwrap(), "/");
        assert_eq!(norm(b"/a/..").unwrap(), "/");
        assert_eq!(norm(b"/a/b/../..").unwrap(), "/");
    }

    #[test]
    fn climbing_above_root_is_refused() {
        assert_eq!(norm(b"../x"), Err(PathError::Escapes));
        assert_eq!(norm(b"/.."), Err(PathError::Escapes));
        assert_eq!(norm(b"/a/../../b"), Err(PathError::Escapes));
    }

    #[test]
    fn nul_bytes_are_refused() {
        assert_eq!(norm(b"/a\0b"), Err(PathError::InvalidByte));
    }

    #[test]
    fn small_output_buffers_are_reported() {
        let mut out = [0u8; 3];
        assert_eq!(normalize(b"/abcd", &mut out), Err(PathError::Truncated));
        let mut out = [0u8; 4];
        assert_eq!(normalize(b"/abc", &mut out), Ok(4));
        assert_eq!(&out, b"/abc");
        let mut none: [u8; 0] = [];
        assert_eq!(normalize(b"/", &mut none), Err(PathError::Truncated));
    }
}
