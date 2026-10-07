//! Image thumbnails in the freedesktop thumbnail cache (see the Thumbnail
//! Managing Standard), shared with Nautilus, Dolphin and others: naming,
//! validity checks, generation, and failure records. UI-independent; the
//! cache root is a parameter so tests never touch `~/.cache`.

use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::UNIX_EPOCH;

use image::{DynamicImage, RgbaImage, imageops::FilterType};
use md5::{Digest, Md5};

/// Formats the thumbnailer decodes (the image previewer's, without svg and
/// ico, which the `image` features here do not cover).
pub const EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff"];
const RAW_EXTENSIONS: &[&str] = crate::raw_preview::EXTENSIONS;

/// Larger files are not decoded for a thumbnail.
pub const MAX_FILE_SIZE: u64 = 64 * 1024 * 1024;

/// Application name in `fail/<app>/` and in the `Software` text chunk.
const APP: &str = "fyle";

/// A thumbnail size directory of the standard.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SizeClass {
    Normal,
    Large,
    XLarge,
    XXLarge,
}

impl SizeClass {
    const ALL: [SizeClass; 4] = [Self::Normal, Self::Large, Self::XLarge, Self::XXLarge];

    /// Longest side of thumbnails in this class.
    pub fn pixels(self) -> u32 {
        match self {
            Self::Normal => 128,
            Self::Large => 256,
            Self::XLarge => 512,
            Self::XXLarge => 1024,
        }
    }

    pub fn dir_name(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Large => "large",
            Self::XLarge => "x-large",
            Self::XXLarge => "xx-large",
        }
    }

    /// The smallest class holding `pixels` (the largest if none does).
    pub fn for_pixels(pixels: u32) -> Self {
        Self::ALL
            .into_iter()
            .find(|class| class.pixels() >= pixels)
            .unwrap_or(Self::XXLarge)
    }
}

/// `file://` URI of an absolute path as GLib writes it: everything except
/// unreserved characters and `!$&'()*+,;=:@/` is percent-encoded (UTF-8).
pub fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for &byte in uri_path(path).as_ref() {
        if byte.is_ascii_alphanumeric() || b"-_.~!$&'()*+,;=:@/".contains(&byte) {
            uri.push(byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

/// The bytes of `path` that follow `file://`: the raw path on Unix.
#[cfg(unix)]
fn uri_path(path: &Path) -> std::borrow::Cow<'_, [u8]> {
    use std::os::unix::ffi::OsStrExt as _;
    path.as_os_str().as_bytes().into()
}

/// The bytes of `path` that follow `file://`, as GLib writes them on
/// Windows: `/C:/dir/file` for drive paths, `server/share/file` for UNC.
#[cfg(not(unix))]
fn uri_path(path: &Path) -> std::borrow::Cow<'_, [u8]> {
    let path = path.to_string_lossy().replace('\\', "/");
    let path = match path.strip_prefix("//?/") {
        Some(verbatim) => match verbatim.strip_prefix("UNC/") {
            Some(unc) => format!("//{unc}"),
            None => verbatim.to_string(),
        },
        None => path,
    };
    let path = match path.strip_prefix("//") {
        Some(unc) => unc.to_string(),
        None if path.starts_with('/') => path,
        None => format!("/{path}"),
    };
    path.into_bytes().into()
}

/// File name of the thumbnail of `uri`: lowercase hex MD5 plus `.png`.
pub fn thumb_name(uri: &str) -> String {
    let digest = Md5::digest(uri.as_bytes());
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{hex}.png")
}

/// The thumbnail cache directory (`$XDG_CACHE_HOME/thumbnails`).
pub fn default_root() -> Option<PathBuf> {
    dirs::cache_dir().map(|dir| dir.join("thumbnails"))
}

/// Why a file has no thumbnail.
#[derive(Debug, PartialEq, Eq)]
pub enum Skip {
    /// Not a regular image file we handle, too large, or inside the cache.
    Ineligible,
    /// Decoding failed before (recorded) or now.
    Failed,
    /// The file or the cache could not be accessed.
    Io,
}

/// Whether `path` (a file of `size` bytes) is worth a thumbnail.
pub fn eligible(path: &Path, size: u64, root: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    ((size <= MAX_FILE_SIZE && EXTENSIONS.contains(&ext.as_str()))
        || RAW_EXTENSIONS.contains(&ext.as_str()))
        && !path.starts_with(root)
}

/// The thumbnail of `path` in `class`, from the cache when valid, else
/// generated and stored. Reads the disk; call from a background thread.
pub fn thumbnail(root: &Path, path: &Path, class: SizeClass) -> Result<RgbaImage, Skip> {
    let meta = fs::metadata(path).map_err(|_| Skip::Io)?;
    if !meta.is_file() || !eligible(path, meta.len(), root) {
        return Err(Skip::Ineligible);
    }
    let mtime = mtime_secs(&meta).ok_or(Skip::Io)?;
    let uri = file_uri(path);
    let name = thumb_name(&uri);
    let cached = root.join(class.dir_name()).join(&name);
    if let Some(image) = read_valid(&cached, &uri, mtime) {
        return Ok(image);
    }
    let failed = root.join("fail").join(APP).join(&name);
    if is_valid(&failed, &uri, mtime) {
        return Err(Skip::Failed);
    }
    match generate(path, class) {
        Ok(image) => {
            // A cache that cannot be written still yields the picture.
            let _ = write_png(&cached, &image, &uri, mtime, Some(class));
            Ok(image)
        }
        Err(Skip::Io) => Err(Skip::Io),
        Err(Skip::Failed) | Err(Skip::Ineligible) => {
            let _ = write_png(&failed, &RgbaImage::new(1, 1), &uri, mtime, None);
            Err(Skip::Failed)
        }
    }
}

/// Modification time in whole seconds (what `Thumb::MTime` holds).
fn mtime_secs(meta: &fs::Metadata) -> Option<i64> {
    let since = meta.modified().ok()?.duration_since(UNIX_EPOCH);
    Some(match since {
        Ok(after) => after.as_secs() as i64,
        Err(before) => -(before.duration().as_secs() as i64),
    })
}

/// Decodes `path` and scales it to fit `class`, never enlarging.
fn generate(path: &Path, class: SizeClass) -> Result<RgbaImage, Skip> {
    let raw = crate::raw_preview::supported(path);
    let source = if raw {
        crate::raw_preview::extract(path).map_err(|error| {
            if error.kind() == io::ErrorKind::InvalidData {
                Skip::Failed
            } else {
                Skip::Io
            }
        })?
    } else {
        path.to_path_buf()
    };
    let image = image::open(source).map_err(|error| match error {
        image::ImageError::IoError(_) => Skip::Io,
        _ => Skip::Failed,
    })?;
    let side = class.pixels();
    let image: DynamicImage = if image.width() > side || image.height() > side {
        image.resize(side, side, FilterType::Triangle)
    } else {
        image
    };
    Ok(image.into_rgba8())
}

/// The `Thumb::URI` and `Thumb::MTime` texts of the PNG at `path`.
fn read_texts(path: &Path) -> Option<(Option<String>, Option<String>)> {
    let decoder = png::Decoder::new(BufReader::new(File::open(path).ok()?));
    let reader = decoder.read_info().ok()?;
    let text = |key: &str| {
        reader
            .info()
            .uncompressed_latin1_text
            .iter()
            .find(|chunk| chunk.keyword == key)
            .map(|chunk| chunk.text.clone())
    };
    Some((text("Thumb::URI"), text("Thumb::MTime")))
}

/// Whether the PNG at `path` records this very file state (the URI is only
/// compared when present, as the standard allows).
fn is_valid(path: &Path, uri: &str, mtime: i64) -> bool {
    match read_texts(path) {
        Some((recorded_uri, Some(recorded))) => {
            recorded.trim().parse::<i64>() == Ok(mtime)
                && recorded_uri.is_none_or(|recorded| recorded == uri)
        }
        _ => false,
    }
}

/// The cached thumbnail at `path` if it is valid.
fn read_valid(path: &Path, uri: &str, mtime: i64) -> Option<RgbaImage> {
    if !is_valid(path, uri, mtime) {
        return None;
    }
    Some(image::open(path).ok()?.into_rgba8())
}

/// Writes `image` as a PNG with the standard's text chunks: to a temporary
/// file (mode 0600) next to the target, then renamed over it, so readers
/// never see a half-written thumbnail. Directories are created with 0700.
fn write_png(
    path: &Path,
    image: &RgbaImage,
    uri: &str,
    mtime: i64,
    class: Option<SizeClass>,
) -> io::Result<()> {
    let dir = path.parent().ok_or(io::ErrorKind::InvalidInput)?;
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)?;
    // Unique per call: several threads write thumbnails at once.
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let tmp = dir.join(format!(
        ".{APP}-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let file = options.open(&tmp)?;
        let mut encoder = png::Encoder::new(BufWriter::new(file), image.width(), image.height());
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut texts = vec![
            ("Thumb::URI", uri.to_string()),
            ("Thumb::MTime", mtime.to_string()),
        ];
        if let Some(class) = class {
            texts.push(("Thumb::Size", class.pixels().to_string()));
        }
        texts.push(("Software", APP.to_string()));
        for (key, text) in texts {
            encoder
                .add_text_chunk(key.to_string(), text)
                .map_err(io::Error::other)?;
        }
        let mut writer = encoder.write_header().map_err(io::Error::other)?;
        writer
            .write_image_data(image.as_raw())
            .map_err(io::Error::other)?;
        writer.finish().map_err(io::Error::other)?;
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::time::{Duration, SystemTime};

    use image::{ImageEncoder, Rgba};
    use tempfile::TempDir;

    use super::{
        MAX_FILE_SIZE, SizeClass, Skip, eligible, file_uri, read_texts, thumb_name, thumbnail,
    };

    fn make_image(path: &std::path::Path, width: u32, height: u32) {
        image::RgbaImage::from_pixel(width, height, Rgba([200, 30, 30, 255]))
            .save(path)
            .expect("test image");
    }

    fn make_raw(path: &std::path::Path, width: u32, height: u32) {
        let pixels = vec![180u8; (width * height * 3) as usize];
        let mut encoded = Cursor::new(Vec::new());
        image::codecs::jpeg::JpegEncoder::new(&mut encoded)
            .write_image(&pixels, width, height, image::ExtendedColorType::Rgb8)
            .expect("encode preview");
        let mut raw = b"RAW wrapper".to_vec();
        raw.extend(encoded.into_inner());
        std::fs::write(path, raw).expect("write raw fixture");
    }

    fn set_mtime(path: &std::path::Path, secs: u64) {
        let time = SystemTime::UNIX_EPOCH + Duration::from_secs(secs);
        std::fs::File::options()
            .write(true)
            .open(path)
            .expect("open")
            .set_modified(time)
            .expect("set mtime");
    }

    #[cfg(windows)]
    #[test]
    fn windows_uri_encoding() {
        use std::path::Path;
        assert_eq!(file_uri(Path::new(r"C:\a b\c.png")), "file:///C:/a%20b/c.png");
        assert_eq!(
            file_uri(Path::new(r"\\?\C:\a\c.png")),
            "file:///C:/a/c.png"
        );
        assert_eq!(
            file_uri(Path::new(r"\\server\share\c.png")),
            "file://server/share/c.png"
        );
        assert_eq!(
            file_uri(Path::new(r"\\?\UNC\server\share\c.png")),
            "file://server/share/c.png"
        );
    }

    #[cfg(unix)]
    #[test]
    fn uri_encoding() {
        use std::path::Path;
        assert_eq!(file_uri(Path::new("/a/b.png")), "file:///a/b.png");
        assert_eq!(
            file_uri(Path::new("/a b/c%d#e?.png")),
            "file:///a%20b/c%25d%23e%3F.png"
        );
        assert_eq!(
            file_uri(Path::new("/é/a+b,c;d=e@f:g'h")),
            "file:///%C3%A9/a+b,c;d=e@f:g'h"
        );
    }

    #[test]
    fn name_is_md5_of_uri() {
        // Checked against md5sum.
        assert_eq!(
            thumb_name("file:///home/jens/photo%20collection/mydomog.jpeg"),
            "3ce3de16497e859c39d94d353e212e51.png"
        );
    }

    #[test]
    fn size_classes() {
        assert_eq!(SizeClass::for_pixels(48), SizeClass::Normal);
        assert_eq!(SizeClass::for_pixels(128), SizeClass::Normal);
        assert_eq!(SizeClass::for_pixels(129), SizeClass::Large);
        assert_eq!(SizeClass::for_pixels(320), SizeClass::XLarge);
        assert_eq!(SizeClass::for_pixels(5000), SizeClass::XXLarge);
    }

    #[test]
    fn generates_scaled_thumbnail_with_text_chunks() {
        let dir = TempDir::new().expect("tmp");
        let root = dir.path().join("cache");
        let file = dir.path().join("pic one.png");
        make_image(&file, 400, 200);
        set_mtime(&file, 1_700_000_000);

        let thumb = thumbnail(&root, &file, SizeClass::Normal).expect("thumbnail");
        assert_eq!((thumb.width(), thumb.height()), (128, 64));

        let stored = root.join("normal").join(thumb_name(&file_uri(&file)));
        let (uri, mtime) = read_texts(&stored).expect("png");
        assert_eq!(uri.as_deref(), Some(file_uri(&file).as_str()));
        assert_eq!(mtime.as_deref(), Some("1700000000"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = |path: &std::path::Path| {
                std::fs::metadata(path).expect("meta").permissions().mode() & 0o777
            };
            assert_eq!(mode(&stored), 0o600);
            assert_eq!(mode(&root.join("normal")), 0o700);
        }
    }

    #[test]
    fn small_images_are_not_enlarged() {
        let dir = TempDir::new().expect("tmp");
        let file = dir.path().join("small.png");
        make_image(&file, 20, 10);
        let thumb = thumbnail(&dir.path().join("cache"), &file, SizeClass::Large).expect("thumb");
        assert_eq!((thumb.width(), thumb.height()), (20, 10));
    }

    #[test]
    fn raw_thumbnail_keeps_original_identity_and_ignores_the_image_size_cap() {
        let dir = TempDir::new().expect("tmp");
        let root = dir.path().join("cache");
        let file = dir.path().join("capture.NEF");
        make_raw(&file, 300, 150);
        std::fs::File::options()
            .write(true)
            .open(&file)
            .expect("open RAW fixture")
            .set_len(MAX_FILE_SIZE + 1)
            .expect("extend sparse RAW fixture");
        set_mtime(&file, 1_700_000_000);
        assert!(eligible(
            &file,
            std::fs::metadata(&file).expect("metadata").len(),
            &root
        ));
        let thumb = thumbnail(&root, &file, SizeClass::Normal).expect("RAW thumbnail");
        assert_eq!((thumb.width(), thumb.height()), (128, 64));
        let stored = root.join("normal").join(thumb_name(&file_uri(&file)));
        let (uri, mtime) = read_texts(&stored).expect("cached thumbnail");
        assert_eq!(uri.as_deref(), Some(file_uri(&file).as_str()));
        assert_eq!(mtime.as_deref(), Some("1700000000"));
        assert!(
            !root
                .join("fail/fyle")
                .join(thumb_name(&file_uri(&file)))
                .exists()
        );
    }

    #[test]
    fn raw_without_preview_records_failure_until_source_mtime_changes() {
        let dir = TempDir::new().expect("tmp");
        let root = dir.path().join("cache");
        let file = dir.path().join("capture.dng");
        std::fs::write(&file, b"no embedded JPEG").expect("write");
        set_mtime(&file, 1_600_000_000);
        assert_eq!(
            thumbnail(&root, &file, SizeClass::Normal),
            Err(Skip::Failed)
        );
        assert!(
            root.join("fail/fyle")
                .join(thumb_name(&file_uri(&file)))
                .is_file()
        );
        make_raw(&file, 20, 10);
        set_mtime(&file, 1_600_000_001);
        assert!(thumbnail(&root, &file, SizeClass::Normal).is_ok());
    }

    #[test]
    fn valid_cache_is_reused_and_stale_is_regenerated() {
        let dir = TempDir::new().expect("tmp");
        let root = dir.path().join("cache");
        let file = dir.path().join("a.png");
        make_image(&file, 300, 300);
        set_mtime(&file, 1_600_000_000);
        thumbnail(&root, &file, SizeClass::Normal).expect("first");
        let stored = root.join("normal").join(thumb_name(&file_uri(&file)));

        // Reused: marking the cached file with an old mtime of our own would
        // be rewritten if it were regenerated.
        let first = std::fs::metadata(&stored)
            .expect("meta")
            .modified()
            .expect("mtime");
        set_mtime(&stored, 1_000);
        thumbnail(&root, &file, SizeClass::Normal).expect("second");
        let reused = std::fs::metadata(&stored)
            .expect("meta")
            .modified()
            .expect("mtime");
        assert_ne!(reused, first);
        assert_eq!(reused, SystemTime::UNIX_EPOCH + Duration::from_secs(1_000));

        // Stale: the source changed, the thumbnail is rewritten.
        set_mtime(&file, 1_600_000_100);
        let thumb = thumbnail(&root, &file, SizeClass::Normal).expect("third");
        assert_eq!(thumb.width(), 128);
        let (_, mtime) = read_texts(&stored).expect("png");
        assert_eq!(mtime.as_deref(), Some("1600000100"));
    }

    #[test]
    fn failures_are_recorded_until_the_file_changes() {
        let dir = TempDir::new().expect("tmp");
        let root = dir.path().join("cache");
        let file = dir.path().join("broken.png");
        std::fs::write(&file, b"not an image").expect("write");
        set_mtime(&file, 1_600_000_000);

        assert_eq!(
            thumbnail(&root, &file, SizeClass::Normal),
            Err(Skip::Failed)
        );
        let record = root.join("fail/fyle").join(thumb_name(&file_uri(&file)));
        assert!(record.is_file());

        // Even a now valid image is not retried while the mtime is the same.
        make_image(&file, 10, 10);
        set_mtime(&file, 1_600_000_000);
        assert_eq!(
            thumbnail(&root, &file, SizeClass::Normal),
            Err(Skip::Failed)
        );

        set_mtime(&file, 1_600_000_001);
        assert!(thumbnail(&root, &file, SizeClass::Normal).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_files_leave_no_failure_record() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().expect("tmp");
        let root = dir.path().join("cache");
        let file = dir.path().join("locked.png");
        make_image(&file, 10, 10);
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).expect("chmod");
        if std::fs::File::open(&file).is_ok() {
            return; // running as root: permissions do not apply
        }
        assert_eq!(thumbnail(&root, &file, SizeClass::Normal), Err(Skip::Io));
        assert!(!root.join("fail").exists());
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).expect("chmod");
        assert!(thumbnail(&root, &file, SizeClass::Normal).is_ok());
    }

    #[test]
    fn concurrent_generation_in_one_directory() {
        let dir = TempDir::new().expect("tmp");
        let root = dir.path().join("cache");
        let files: Vec<_> = (0..8)
            .map(|i| {
                let file = dir.path().join(format!("{i}.png"));
                make_image(&file, 200, 100);
                file
            })
            .collect();
        std::thread::scope(|scope| {
            for file in &files {
                let root = &root;
                scope.spawn(move || {
                    assert!(thumbnail(root, file, SizeClass::Normal).is_ok());
                });
            }
        });
        let stored = std::fs::read_dir(root.join("normal")).expect("dir").count();
        assert_eq!(stored, 8, "one thumbnail per file, no temporary files left");
    }

    #[test]
    fn ineligible_files_are_skipped() {
        let dir = TempDir::new().expect("tmp");
        let root = dir.path().join("cache");
        // A file inside the cache itself.
        let inside = root.join("normal/x.png");
        std::fs::create_dir_all(inside.parent().expect("parent")).expect("dirs");
        make_image(&inside, 10, 10);
        assert_eq!(
            thumbnail(&root, &inside, SizeClass::Normal),
            Err(Skip::Ineligible)
        );
        // Not an image, a directory, and a missing file.
        let text = dir.path().join("a.txt");
        std::fs::write(&text, "x").expect("write");
        assert_eq!(
            thumbnail(&root, &text, SizeClass::Normal),
            Err(Skip::Ineligible)
        );
        assert_eq!(
            thumbnail(&root, dir.path(), SizeClass::Normal),
            Err(Skip::Ineligible)
        );
        assert_eq!(
            thumbnail(&root, &dir.path().join("none.png"), SizeClass::Normal),
            Err(Skip::Io)
        );
        // Too large, and svg.
        assert!(!eligible(
            &dir.path().join("a.png"),
            MAX_FILE_SIZE + 1,
            &root
        ));
        assert!(!eligible(&dir.path().join("a.svg"), 10, &root));
        assert!(eligible(&dir.path().join("A.JPG"), 10, &root));
        assert!(!root.join("fail").exists());
    }
}
