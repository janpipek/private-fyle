//! Extended attributes (Linux): names for the Xattrs column and the
//! SELinux context. Symlinks are not followed.

use std::path::Path;

/// Names of the entry's extended attributes, sorted.
pub fn names(path: &Path) -> Vec<String> {
    let mut names: Vec<String> = imp::list(path)
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
    let value = imp::get(path, "security.selinux")?;
    let text = String::from_utf8_lossy(&value);
    Some(text.trim_end_matches('\0').to_string()).filter(|t| !t.is_empty())
}

#[cfg(target_os = "linux")]
mod imp {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    fn c_path(path: &Path) -> Option<CString> {
        CString::new(path.as_os_str().as_bytes()).ok()
    }

    /// Calls `f(buffer, len)` (a size query first, with a null buffer).
    fn read(mut f: impl FnMut(*mut libc::c_void, usize) -> isize) -> Option<Vec<u8>> {
        loop {
            let size = f(std::ptr::null_mut(), 0);
            if size < 0 {
                return None;
            }
            let mut buf = vec![0u8; size as usize];
            let read = f(buf.as_mut_ptr().cast(), buf.len());
            if read >= 0 {
                buf.truncate(read as usize);
                return Some(buf);
            }
            // Grew in between: ask again.
            if std::io::Error::last_os_error().raw_os_error() != Some(libc::ERANGE) {
                return None;
            }
        }
    }

    pub fn list(path: &Path) -> Option<Vec<u8>> {
        let path = c_path(path)?;
        // SAFETY: `path` is a valid C string; `buf`/`len` describe a writable
        // buffer (or are null/0 for the size query).
        read(|buf, len| unsafe { libc::llistxattr(path.as_ptr(), buf.cast(), len) })
    }

    pub fn get(path: &Path, name: &str) -> Option<Vec<u8>> {
        let (path, name) = (c_path(path)?, CString::new(name).ok()?);
        // SAFETY: as in `list`.
        read(|buf, len| unsafe { libc::lgetxattr(path.as_ptr(), name.as_ptr(), buf, len) })
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use std::path::Path;

    pub fn list(_: &Path) -> Option<Vec<u8>> {
        None
    }

    pub fn get(_: &Path, _: &str) -> Option<Vec<u8>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_user_attributes() {
        let path = std::env::temp_dir().join(format!("fyle-xattr-{}", std::process::id()));
        std::fs::write(&path, "").unwrap();
        assert!(!names(&path).contains(&"user.fyle".to_string()));
        let set = std::process::Command::new("setfattr")
            .args(["-n", "user.fyle", "-v", "1"])
            .arg(&path)
            .status();
        // Needs setfattr and a file system with user xattrs.
        if set.is_ok_and(|s| s.success()) {
            assert!(names(&path).contains(&"user.fyle".to_string()));
        }
        assert!(names(Path::new("/no/such/file")).is_empty());
        std::fs::remove_file(&path).unwrap();
    }
}
