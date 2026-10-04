//! A text file being edited, UI-independent: loading, change tracking and
//! saving. The editor view (`editor_view.rs`) owns the text buffer.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::preview::text::language_for;

/// Larger files are refused; the editor handles about 50K lines well.
pub const MAX_EDIT_SIZE: u64 = 8 * 1024 * 1024;

pub struct Document {
    path: PathBuf,
    /// Content as last loaded or saved.
    saved: String,
    language: Option<&'static str>,
}

impl Document {
    /// Opens a UTF-8 text file. Anything else is refused rather than
    /// converted, since saving a lossy conversion would corrupt the file.
    pub fn open(path: &Path) -> Result<Self, String> {
        let error = |msg: &dyn std::fmt::Display| format!("{}: {msg}", path.display());
        let meta = fs::metadata(path).map_err(|e| error(&e))?;
        if !meta.is_file() {
            return Err(error(&"not a file"));
        }
        if meta.len() > MAX_EDIT_SIZE {
            return Err(error(&format!(
                "too large to edit ({} MiB max)",
                MAX_EDIT_SIZE / 1024 / 1024
            )));
        }
        let bytes = fs::read(path).map_err(|e| error(&e))?;
        if bytes.contains(&0) {
            return Err(error(&"binary file"));
        }
        let saved = String::from_utf8(bytes).map_err(|_| error(&"not UTF-8 text"))?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        Ok(Self {
            path: path.to_path_buf(),
            saved,
            language: language_for(&name, &ext),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    pub fn text(&self) -> &str {
        &self.saved
    }

    /// Highlighting language, `"text"` if unknown.
    pub fn language(&self) -> &'static str {
        self.language.unwrap_or("text")
    }

    pub fn is_modified(&self, current: &str) -> bool {
        current != self.saved
    }

    /// Writes `text` via a temporary file in the same directory and a rename,
    /// so a failed save never leaves a truncated file. Keeps permissions.
    pub fn save(&mut self, text: &str) -> Result<(), String> {
        let error = |e: std::io::Error| format!("{}: {e}", self.path.display());
        let dir = self.path.parent().unwrap_or(Path::new("."));
        let tmp = dir.join(format!(".{}.fyle-save", self.name()));
        let result = (|| {
            let mut file = fs::File::create(&tmp)?;
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            if let Ok(meta) = fs::metadata(&self.path) {
                fs::set_permissions(&tmp, meta.permissions())?;
            }
            fs::rename(&tmp, &self.path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result.map_err(error)?;
        self.saved = text.to_string();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fyle-editor-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn opens_text_and_refuses_others() {
        let dir = temp("open");
        fs::write(dir.join("a.rs"), "fn main() {}\n").unwrap();
        fs::write(dir.join("bin"), [b'a', 0, b'b']).unwrap();
        fs::write(dir.join("latin1.txt"), [b'c', 0xe9]).unwrap();

        let doc = Document::open(&dir.join("a.rs")).unwrap();
        assert_eq!(doc.text(), "fn main() {}\n");
        assert_eq!(doc.language(), "rust");
        assert!(!doc.is_modified("fn main() {}\n"));
        assert!(doc.is_modified("fn main() {}"));

        let err = |name: &str| Document::open(&dir.join(name)).err().unwrap();
        assert!(err("bin").ends_with("binary file"));
        assert!(err("latin1.txt").ends_with("not UTF-8 text"));
        assert!(err("missing").contains("missing"));
        assert!(Document::open(&dir).is_err()); // a directory
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn saves_atomically_keeping_permissions() {
        let dir = temp("save");
        let path = dir.join("run.sh");
        fs::write(&path, "echo 1\r\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let mut doc = Document::open(&path).unwrap();
        doc.save("echo 2\r\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "echo 2\r\n"); // CRLF kept
        assert!(!doc.is_modified("echo 2\r\n"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o755
            );
        }
        // No temporary file left behind.
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }
}
