//! Bounded native extended-attribute reads (Linux), without following symbolic links.

/// UI-independent, cancellable attribute-value inspection.
pub mod inspection;

use std::io;
use std::path::Path;

const NATIVE_LIMIT: usize = 64 * 1024;
#[cfg(any(target_os = "linux", test))]
const ATTEMPTS: usize = 3;

/// A bounded native value or its queried size when the read would exceed the budget.
pub enum ReadValue {
    /// Complete value returned by the native API.
    Complete(Vec<u8>),
    /// Value omitted without allocating its declared size.
    Omitted(usize),
}

/// Names of the entry's extended attributes, sorted; unavailable reads remain blank.
pub fn names(path: &Path) -> Vec<String> {
    let mut names: Vec<String> = list(path, &mut || Ok(()))
        .unwrap_or_default()
        .split(|&b| b == 0)
        .filter(|n| !n.is_empty())
        .map(|n| String::from_utf8_lossy(n).into_owned())
        .collect();
    names.sort();
    names
}

/// The SELinux context, e.g. `unconfined_u:object_r:user_home_t:s0`.
pub fn selinux_context(path: &Path) -> Option<String> {
    let ReadValue::Complete(value) =
        get(path, b"security.selinux", NATIVE_LIMIT, &mut || Ok(())).ok()?
    else {
        return None;
    };
    let text = String::from_utf8_lossy(&value);
    Some(text.trim_end_matches('\0').to_string()).filter(|t| !t.is_empty())
}

/// Reads the bounded, NUL-separated native name list, keeping failures visible to callers.
pub fn list(path: &Path, check: &mut dyn FnMut() -> io::Result<()>) -> io::Result<Vec<u8>> {
    match imp::list(path, check)? {
        ReadValue::Complete(bytes) => Ok(bytes),
        ReadValue::Omitted(_) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "extended attribute name list exceeds 64 KiB",
        )),
    }
}

/// Reads one raw attribute name with an allocation cap and checks between native calls.
pub fn get(
    path: &Path,
    name: &[u8],
    limit: usize,
    check: &mut dyn FnMut() -> io::Result<()>,
) -> io::Result<ReadValue> {
    imp::get(path, name, limit.min(NATIVE_LIMIT), check)
}

/// Whether the filesystem/platform does not support extended attributes.
pub fn unsupported(error: &io::Error) -> bool {
    if error.kind() == io::ErrorKind::Unsupported {
        return true;
    }
    #[cfg(target_os = "linux")]
    if error.raw_os_error() == Some(libc::ENOTSUP) {
        return true;
    }
    false
}

#[cfg(any(target_os = "linux", test))]
fn read_bounded(
    mut call: impl FnMut(*mut u8, usize) -> io::Result<usize>,
    limit: usize,
) -> io::Result<ReadValue> {
    for _ in 0..ATTEMPTS {
        let size = call(std::ptr::null_mut(), 0)?;
        if size > limit {
            return Ok(ReadValue::Omitted(size));
        }
        if size == 0 {
            return Ok(ReadValue::Complete(Vec::new()));
        }
        let mut bytes = vec![0; size];
        match call(bytes.as_mut_ptr(), bytes.len()) {
            Ok(count) if count <= bytes.len() => {
                bytes.truncate(count);
                return Ok(ReadValue::Complete(bytes));
            }
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "native attribute read exceeded its buffer",
                ));
            }
            Err(error) if is_range(&error) => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other(
        "extended attributes changed repeatedly during inspection",
    ))
}

#[cfg(any(target_os = "linux", test))]
fn is_range(error: &io::Error) -> bool {
    // Linux's ERANGE is 34; non-Linux builds only exercise the pure retry model in tests.
    error.raw_os_error() == Some(34)
}

#[cfg(target_os = "linux")]
mod imp {
    use std::ffi::CString;
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    use super::{NATIVE_LIMIT, ReadValue, read_bounded};

    fn c_path(path: &Path) -> io::Result<CString> {
        CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))
    }

    fn result(count: isize) -> io::Result<usize> {
        if count < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(count as usize)
        }
    }

    /// Lists attributes of the path object, not its symlink target.
    pub fn list(path: &Path, check: &mut dyn FnMut() -> io::Result<()>) -> io::Result<ReadValue> {
        let path = c_path(path)?;
        read_bounded(
            |bytes, size| {
                check()?;
                // SAFETY: path is NUL-terminated and bytes is either null/0 or a writable buffer of size bytes.
                result(unsafe { libc::llistxattr(path.as_ptr(), bytes.cast(), size) })
            },
            NATIVE_LIMIT,
        )
    }

    /// Retrieves an exact byte-oriented attribute name without following symlinks.
    pub fn get(
        path: &Path,
        name: &[u8],
        limit: usize,
        check: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<ReadValue> {
        let (path, name) = (
            c_path(path)?,
            CString::new(name).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "attribute name contains NUL")
            })?,
        );
        read_bounded(
            |bytes, size| {
                check()?;
                // SAFETY: both strings are NUL-terminated and bytes/size describe the writable buffer or null/0.
                result(unsafe { libc::lgetxattr(path.as_ptr(), name.as_ptr(), bytes.cast(), size) })
            },
            limit,
        )
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use super::ReadValue;
    use std::io;
    use std::path::Path;

    /// No native attribute reader is enabled on this platform.
    pub fn list(_: &Path, _: &mut dyn FnMut() -> io::Result<()>) -> io::Result<ReadValue> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "extended attributes are unavailable on this platform",
        ))
    }
    /// No native attribute reader is enabled on this platform.
    pub fn get(
        _: &Path,
        _: &[u8],
        _: usize,
        _: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<ReadValue> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "extended attributes are unavailable on this platform",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{ATTEMPTS, ReadValue, read_bounded};
    use std::io;

    #[test]
    fn native_size_queries_never_allocate_past_the_cap_and_zero_is_a_value() {
        let mut calls = 0;
        let value = read_bounded(
            |_, size| {
                calls += 1;
                assert_eq!(size, 0);
                Ok(usize::MAX)
            },
            1024,
        )
        .unwrap();
        assert!(matches!(value, ReadValue::Omitted(usize::MAX)));
        assert_eq!(calls, 1);
        assert!(
            matches!(read_bounded(|_, _| Ok(0), 0).unwrap(), ReadValue::Complete(bytes) if bytes.is_empty())
        );
    }

    #[test]
    fn native_growth_retries_are_finite_and_non_range_errors_are_preserved() {
        let mut calls = 0;
        assert!(
            read_bounded(
                |_, size| {
                    calls += 1;
                    if size == 0 {
                        Ok(1)
                    } else {
                        Err(io::Error::from_raw_os_error(34))
                    }
                },
                1024
            )
            .is_err()
        );
        assert_eq!(calls, ATTEMPTS * 2);
        let error = read_bounded(
            |_, _| Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied")),
            1024,
        )
        .err()
        .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }
}

#[cfg(all(test, target_os = "linux"))]
mod native_tests {
    //! Native integration checks use libc directly, without optional command-line programs.

    use std::ffi::CString;
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    use super::{ReadValue, get, inspection, names};
    use crate::properties::{PropertyValue, Registry};

    fn set(path: &Path, name: &[u8], value: &[u8]) -> io::Result<()> {
        let path =
            CString::new(path.as_os_str().as_bytes()).expect("temporary test paths contain no NUL");
        let name = CString::new(name).expect("test attribute names contain no NUL");
        // SAFETY: path/name are NUL-terminated, value describes readable bytes, and flags=0 sets or replaces.
        let result = unsafe {
            libc::lsetxattr(
                path.as_ptr(),
                name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
            )
        };
        if result < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    #[test]
    fn reads_native_text_binary_empty_values_and_directory_properties() {
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("file");
        std::fs::write(&path, "plain text").unwrap();
        match set(&path, b"user.fyle.text", b"hello\nworld") {
            Err(error) if super::unsupported(&error) => return,
            result => result.unwrap(),
        }
        set(&path, b"user.fyle.binary", &[0, 255, 65]).unwrap();
        set(&path, b"user.fyle.empty", &[]).unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let snapshot = inspection::read(&path, &cancel).unwrap();
        let binary = snapshot
            .attributes
            .iter()
            .find(|attr| attr.name == "user.fyle.binary")
            .unwrap();
        assert!(
            matches!(&binary.value, inspection::Value::Ready { preview: inspection::Preview::Hex(hex), .. } if hex == "00 ff 41")
        );
        assert!(names(&path).contains(&"user.fyle.text".to_string()));
        assert!(
            matches!(get(&path, b"user.fyle.empty", 0, &mut || Ok(())).unwrap(), ReadValue::Complete(bytes) if bytes.is_empty())
        );
        assert!(get(&path, b"user.fyle.missing", 1024, &mut || Ok(())).is_err());
        assert!(names(&temp.path().join("missing")).is_empty());
        for selected in [path.as_path(), temp.path()] {
            if selected == temp.path() {
                set(selected, b"user.fyle.text", b"directory metadata").unwrap();
            }
            let groups = Registry::with_defaults().read(selected, &cancel).unwrap();
            let attrs = groups
                .iter()
                .find(|group| group.id.0 == "extended_attributes")
                .unwrap();
            assert!(attrs.groups.iter().any(|group| {
                group.fields.iter().any(|field| {
                    field.id == "name"
                        && field.value == PropertyValue::Text("user.fyle.text".into())
                })
            }));
        }
    }

    #[test]
    fn symlinks_never_inherit_target_attributes_and_broken_links_can_be_probed() {
        let temp = tempfile::TempDir::new().unwrap();
        let target = temp.path().join("target");
        std::fs::write(&target, "target").unwrap();
        match set(&target, b"user.fyle.target", b"target-only value") {
            Err(error) if super::unsupported(&error) => return,
            result => result.unwrap(),
        }
        let link = temp.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        assert!(
            !inspection::read(&link, &cancel)
                .unwrap()
                .attributes
                .iter()
                .any(|attr| attr.name == "user.fyle.target")
        );
        std::fs::remove_file(&target).unwrap();
        assert!(
            !inspection::read(&link, &cancel)
                .unwrap()
                .attributes
                .iter()
                .any(|attr| attr.name == "user.fyle.target")
        );
        assert!(Registry::with_defaults().read(&link, &cancel).is_ok());
    }
}
