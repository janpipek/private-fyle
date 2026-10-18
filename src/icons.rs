//! File icons, UI-independent: maps an entry to a semantic [`FileIcon`].
//! The view decides which glyph draws each kind, so adding a file type is a
//! table entry here, and a new kind needs one more glyph in the view.

/// What an entry's icon depicts. Also used as the file "kind" that tools
/// match on (`kinds = ["image"]` in tool definitions).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileIcon {
    ParentDir,
    Folder,
    FolderOpen,
    FolderGit,
    File,
    Code,
    /// Structured data: JSON, YAML, TOML, XML, …
    Data,
    Text,
    Image,
    Audio,
    Video,
    Archive,
    Spreadsheet,
    /// Build and tool configuration.
    Config,
    Lock,
    Key,
    Script,
    Font,
    Diff,
}

impl FileIcon {
    #[cfg(test)]
    pub const ALL: [FileIcon; 19] = [
        Self::ParentDir,
        Self::Folder,
        Self::FolderOpen,
        Self::FolderGit,
        Self::File,
        Self::Code,
        Self::Data,
        Self::Text,
        Self::Image,
        Self::Audio,
        Self::Video,
        Self::Archive,
        Self::Spreadsheet,
        Self::Config,
        Self::Lock,
        Self::Key,
        Self::Script,
        Self::Font,
        Self::Diff,
    ];
}

/// Icon for an entry. `expanded` only matters for directories in tree mode.
pub fn icon_for(name: &str, is_dir: bool, is_parent: bool, expanded: bool) -> FileIcon {
    if is_parent {
        return FileIcon::ParentDir;
    }
    if is_dir {
        return match name {
            ".git" => FileIcon::FolderGit,
            _ if expanded => FileIcon::FolderOpen,
            _ => FileIcon::Folder,
        };
    }
    by_name(name)
        .or_else(|| {
            let (_, ext) = name.rsplit_once('.').filter(|(stem, _)| !stem.is_empty())?;
            by_extension(&ext.to_lowercase())
        })
        .unwrap_or(FileIcon::File)
}

/// Uses background content evidence when available, preserving filename-specific icons otherwise.
pub fn icon_for_detected(
    name: &str,
    is_dir: bool,
    is_parent: bool,
    expanded: bool,
    candidate: Option<&crate::recognition::Candidate>,
) -> FileIcon {
    let fallback = icon_for(name, is_dir, is_parent, expanded);
    if is_dir || is_parent {
        return fallback;
    }
    let detected = candidate.cloned().or_else(|| {
        crate::recognition::detect(name, false, None, None, &crate::recognition::NativeDetector)
            .into_iter()
            .next()
    });
    let Some(candidate) = detected.filter(|c| c.evidence > crate::recognition::Evidence::Fallback)
    else {
        return fallback;
    };
    let mime = candidate.mime.as_str();
    if mime.starts_with("image/") {
        return FileIcon::Image;
    }
    if mime.starts_with("audio/") {
        return FileIcon::Audio;
    }
    if mime.starts_with("video/") {
        return FileIcon::Video;
    }
    match mime {
        "application/vnd.sqlite3"
        | "application/rdf+xml"
        | "application/yaml"
        | "application/json"
        | "application/xml"
        | "application/vnd.apache.parquet" => FileIcon::Data,
        "text/x-glsl" | "text/wgsl" | "text/x-hlsl" => FileIcon::Code,
        "application/pdf" => FileIcon::Text,
        "application/zip" | "application/x-rpm" | "application/vnd.debian.binary-package" => {
            FileIcon::Archive
        }
        _ => fallback,
    }
}

fn by_name(name: &str) -> Option<FileIcon> {
    Some(match name {
        "Cargo.lock" | "package-lock.json" | "yarn.lock" | "pnpm-lock.yaml" | "poetry.lock"
        | "uv.lock" | "Gemfile.lock" | "composer.lock" | "flake.lock" => FileIcon::Lock,
        "Makefile" | "makefile" | "GNUmakefile" | "CMakeLists.txt" | "Dockerfile"
        | "Containerfile" | "Justfile" | "justfile" | ".gitignore" | ".gitattributes"
        | ".gitmodules" | ".editorconfig" | ".dockerignore" | ".env" => FileIcon::Config,
        "LICENSE" | "LICENCE" | "COPYING" | "README" | "CHANGELOG" | "AUTHORS" => FileIcon::Text,
        _ => return None,
    })
}

fn by_extension(ext: &str) -> Option<FileIcon> {
    Some(match ext {
        "rs" | "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "hh" | "cs" | "go" | "java" | "kt"
        | "kts" | "scala" | "swift" | "py" | "pyi" | "rb" | "php" | "lua" | "js" | "mjs"
        | "cjs" | "ts" | "mts" | "cts" | "tsx" | "jsx" | "html" | "htm" | "css" | "scss"
        | "sass" | "less" | "vue" | "svelte" | "zig" | "ex" | "exs" | "erl" | "hs" | "ml"
        | "clj" | "dart" | "r" | "jl" | "sql" | "proto" | "graphql" | "gql" => FileIcon::Code,
        "json" | "jsonc" | "json5" | "yaml" | "yml" | "toml" | "xml" | "ron" | "csv" | "tsv"
        | "ndjson" => FileIcon::Data,
        "ini" | "cfg" | "conf" | "config" | "properties" | "env" | "cmake" | "mk" => {
            FileIcon::Config
        }
        "txt" | "md" | "markdown" | "rst" | "adoc" | "org" | "tex" | "log" | "pdf" | "doc"
        | "docx" | "docm" | "dotx" | "dotm" | "odt" | "ott" | "rtf" | "epub" => FileIcon::Text,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" | "ico" | "tif" | "tiff"
        | "heic" | "avif" | "psd" | "xcf" | "raw" | "cr2" | "cr3" | "nef" | "nrw" | "arw"
        | "sr2" | "srf" | "raf" | "rw2" | "orf" | "pef" | "dng" => FileIcon::Image,
        "mp3" | "flac" | "ogg" | "opus" | "wav" | "m4a" | "aac" | "wma" | "aiff" => FileIcon::Audio,
        "mp4" | "mkv" | "webm" | "mov" | "avi" | "wmv" | "m4v" | "flv" => FileIcon::Video,
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "7z" | "rar" | "jar" | "deb"
        | "rpm" | "dmg" | "iso" | "crate" | "whl" => FileIcon::Archive,
        "xls" | "xlsx" | "xlsm" | "xlsb" | "ods" | "numbers" => FileIcon::Spreadsheet,
        "lock" => FileIcon::Lock,
        "pem" | "key" | "crt" | "cer" | "pub" | "gpg" | "asc" | "p12" | "pfx" => FileIcon::Key,
        "sh" | "bash" | "zsh" | "fish" | "ps1" | "bat" | "cmd" | "nu" => FileIcon::Script,
        "ttf" | "otf" | "woff" | "woff2" => FileIcon::Font,
        "diff" | "patch" => FileIcon::Diff,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str) -> FileIcon {
        icon_for(name, false, false, false)
    }

    #[test]
    fn shared_recognition_refines_icons_and_preserves_directory_semantics() {
        use crate::recognition::{NativeDetector, detect};
        let detected = detect(
            "unknown",
            false,
            Some(b"SQLite format 3\0"),
            None,
            &NativeDetector,
        );
        assert_eq!(
            super::icon_for_detected("unknown", false, false, false, detected.first()),
            FileIcon::Data
        );
        assert_eq!(
            super::icon_for_detected("folder", true, false, false, detected.first()),
            FileIcon::Folder
        );
        assert_eq!(
            super::icon_for_detected("shader.wgsl", false, false, false, None),
            FileIcon::Code
        );
    }

    #[test]
    fn directories() {
        assert_eq!(icon_for("..", true, true, false), FileIcon::ParentDir);
        assert_eq!(icon_for("src", true, false, false), FileIcon::Folder);
        assert_eq!(icon_for("src", true, false, true), FileIcon::FolderOpen);
        assert_eq!(icon_for(".git", true, false, false), FileIcon::FolderGit);
        // Directory names never get file icons.
        assert_eq!(icon_for("pics.png", true, false, false), FileIcon::Folder);
    }

    #[test]
    fn files_by_name_then_extension() {
        assert_eq!(file("main.rs"), FileIcon::Code);
        assert_eq!(file("PHOTO.JPG"), FileIcon::Image);
        assert_eq!(file("capture.CR3"), FileIcon::Image);
        assert_eq!(file("backup.tar.gz"), FileIcon::Archive);
        assert_eq!(file("Cargo.lock"), FileIcon::Lock);
        assert_eq!(file("Cargo.toml"), FileIcon::Data);
        assert_eq!(file("Makefile"), FileIcon::Config);
        assert_eq!(file("LICENSE"), FileIcon::Text);
        assert_eq!(file("id_ed25519.pub"), FileIcon::Key);
    }

    #[test]
    fn unknown_and_dotfiles() {
        assert_eq!(file("blob"), FileIcon::File);
        assert_eq!(file("data.xyz"), FileIcon::File);
        // A leading dot is not an extension...
        assert_eq!(file(".bashrc"), FileIcon::File);
        // ...unless the name is known.
        assert_eq!(file(".gitignore"), FileIcon::Config);
    }
}
