//! Safe extraction of selected archive entries into a destination folder.

use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use super::{Cleaned, Kind, Raw, clean, format_of, walk};

/// Counts extracted files and records entries skipped with a reason.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ExtractReport {
    /// Files and links written.
    pub files: usize,
    /// Entries not extracted, with the reason.
    pub skipped: Vec<String>,
}

/// Extracts the entries at or below each of `wanted` (inner paths; `""` is
/// everything) into the directory `dest`, which must exist. Each wanted
/// entry lands directly in `dest` under its own name; for `""` the entries
/// land under `prefix` (or directly in `dest`). `step` is called with each
/// written path and can abort by returning an error.
///
/// Nothing is ever written outside `dest`: names that are absolute or
/// contain `..` are skipped, symbolic links must stay inside `dest`, and no
/// path is written through a symbolic link.
pub fn extract(
    file: &Path,
    wanted: &[String],
    prefix: Option<&str>,
    dest: &Path,
    step: &mut dyn FnMut(&str) -> io::Result<()>,
) -> io::Result<ExtractReport> {
    let format = format_of(file)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a supported archive"))?;
    let mut report = ExtractReport::default();
    let is_wanted = |name: &str| match clean(name) {
        Cleaned::Path(path) => wanted
            .iter()
            .any(|want| relative_to(&path, want, prefix).is_some()),
        _ => false,
    };
    let mut links = Vec::new();
    walk(file, format, None, &is_wanted, &mut |raw, data| {
        let path = match clean(&raw.name) {
            Cleaned::Root => return Ok(true),
            Cleaned::Unsafe => {
                report
                    .skipped
                    .push(format!("{}: unsafe path", raw.name.escape_debug()));
                return Ok(true);
            }
            Cleaned::Path(path) => path,
        };
        for want in wanted {
            let Some(rel) = relative_to(&path, want, prefix) else {
                continue;
            };
            if let Some(problem) = &raw.problem {
                report.skipped.push(format!("{rel}: {problem}"));
                continue;
            }
            match write_entry(dest, &rel, &raw, data) {
                Ok(Written::Done) => {
                    if matches!(raw.kind, Kind::Symlink(_)) {
                        links.push(rel.clone());
                    }
                    report.files += 1;
                    step(&rel)?;
                }
                Ok(Written::Dir) => {}
                Ok(Written::Skipped(why)) => report.skipped.push(format!("{rel}: {why}")),
                Err(e) => return Err(io::Error::new(e.kind(), format!("{rel}: {e}"))),
            }
        }
        Ok(true)
    })?;
    prune_escaping_links(dest, &links, &mut report);
    Ok(report)
}

/// Removes created links that resolve outside `dest`. The lexical check at
/// creation misses chains such as `l1 -> .` then `l2 -> l1/..`; once all
/// entries are written the real targets can be resolved.
fn prune_escaping_links(dest: &Path, links: &[String], report: &mut ExtractReport) {
    let Ok(root) = fs::canonicalize(dest) else {
        return;
    };
    for rel in links {
        let path = dest.join(rel);
        // A dangling link cannot be resolved; it leads nowhere.
        let Ok(resolved) = fs::canonicalize(&path) else {
            continue;
        };
        if !resolved.starts_with(&root) && fs::remove_file(&path).is_ok() {
            report.files = report.files.saturating_sub(1);
            report
                .skipped
                .push(format!("{rel}: link points outside the extraction folder"));
        }
    }
}

/// Where the entry `path` goes when `want` is extracted, relative to the
/// destination; None if it is not at or below `want`.
fn relative_to(path: &str, want: &str, prefix: Option<&str>) -> Option<String> {
    let rel = if want.is_empty() {
        path
    } else {
        let base = want.rsplit_once('/').map_or("", |(base, _)| base);
        let below = path == want
            || path
                .strip_prefix(want)
                .is_some_and(|rest| rest.starts_with('/'));
        if !below {
            return None;
        }
        path[base.len()..].trim_start_matches('/')
    };
    Some(match prefix {
        Some(prefix) if !prefix.is_empty() => format!("{prefix}/{rel}"),
        _ => rel.to_string(),
    })
}

/// The inverse of [`extract`]'s layout: the inner path in the archive of
/// the file that was written to `rel` (relative to the destination), given
/// the same `wanted` and `prefix`. None if `rel` matches nothing wanted.
pub fn entry_of(rel: &str, wanted: &[String], prefix: Option<&str>) -> Option<String> {
    let rel = match prefix {
        Some(prefix) if !prefix.is_empty() => rel.strip_prefix(prefix)?.strip_prefix('/')?,
        _ => rel,
    };
    let first = rel.split('/').next().unwrap_or_default();
    wanted.iter().find_map(|want| {
        if want.is_empty() {
            return Some(rel.to_string());
        }
        let (base, last) = want.rsplit_once('/').unwrap_or(("", want));
        (last == first).then(|| {
            if base.is_empty() {
                rel.to_string()
            } else {
                format!("{base}/{rel}")
            }
        })
    })
}

enum Written {
    Done,
    Dir,
    Skipped(&'static str),
}

fn write_entry(dest: &Path, rel: &str, raw: &Raw, data: &mut dyn Read) -> io::Result<Written> {
    let target = safe_join(dest, rel)?;
    match &raw.kind {
        Kind::Dir => {
            fs::create_dir_all(&target)?;
            set_mode(&target, raw.mode.map(|m| m | 0o700));
            Ok(Written::Dir)
        }
        Kind::File => {
            prepare_parent(&target)?;
            remove_existing(&target)?;
            let mut out = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)?;
            io::copy(data, &mut out)?;
            if let Some(time) = raw.modified {
                let _ = out.set_modified(time);
            }
            drop(out);
            set_mode(&target, raw.mode.map(|m| m | 0o600));
            Ok(Written::Done)
        }
        Kind::Symlink(link) => {
            if !link_stays_inside(rel, link) {
                return Ok(Written::Skipped(
                    "link points outside the extraction folder",
                ));
            }
            prepare_parent(&target)?;
            remove_existing(&target)?;
            make_symlink(link, &target)
        }
        Kind::Other => Ok(Written::Skipped("unsupported entry type")),
    }
}

#[cfg(unix)]
fn make_symlink(link: &str, target: &Path) -> io::Result<Written> {
    std::os::unix::fs::symlink(link, target)?;
    Ok(Written::Done)
}

#[cfg(not(unix))]
fn make_symlink(_: &str, _: &Path) -> io::Result<Written> {
    Ok(Written::Skipped("links are not supported here"))
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: Option<u32>) {
    use std::os::unix::fs::PermissionsExt;
    if let Some(mode) = mode {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o777));
    }
}

#[cfg(not(unix))]
fn set_mode(_: &Path, _: Option<u32>) {}

/// `dest` joined with `rel`, which must consist of normal components only
/// and not pass through an existing symbolic link.
fn safe_join(dest: &Path, rel: &str) -> io::Result<PathBuf> {
    let mut path = dest.to_path_buf();
    for component in Path::new(rel).components() {
        let Component::Normal(part) = component else {
            return Err(io::Error::other("unsafe path"));
        };
        path.push(part);
        // Every directory on the way must be a real one; the last
        // component is checked by its writer.
        if path != dest.join(rel)
            && fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink())
        {
            return Err(io::Error::other("path leads through a symbolic link"));
        }
    }
    Ok(path)
}

/// Creates the parent directories of `target`.
fn prepare_parent(target: &Path) -> io::Result<()> {
    match target.parent() {
        Some(parent) => fs::create_dir_all(parent),
        None => Ok(()),
    }
}

/// Removes a file or link in the way (a repeated entry name); never a
/// directory.
fn remove_existing(target: &Path) -> io::Result<()> {
    match fs::symlink_metadata(target) {
        Ok(meta) if meta.is_dir() => Err(io::Error::other("a folder is in the way")),
        Ok(_) => fs::remove_file(target),
        Err(_) => Ok(()),
    }
}

/// Whether a link at `rel` (relative to the destination) pointing to
/// `target` stays inside the destination: relative, and never above it.
fn link_stays_inside(rel: &str, target: &str) -> bool {
    if target.is_empty()
        || target.starts_with('/')
        || target.starts_with('\\')
        || target.contains('\0')
        || target.as_bytes().get(1) == Some(&b':')
    {
        return false;
    }
    let mut depth = rel.split('/').count() as i64 - 1;
    for part in target.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => depth += 1,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::{entry_of, extract, link_stays_inside, relative_to};
    use crate::archive::tests::{Item, SAMPLE, all_formats, make_tar, make_zip};
    use crate::archive::{location, read_file};
    use std::fs::{self, File};
    use std::io::{self, Write};
    use std::path::Path;
    use tempfile::TempDir;

    #[test]
    fn extracts_selected_entries_by_name() {
        let dir = TempDir::new().unwrap();
        for (n, file) in all_formats(&dir, SAMPLE).into_iter().enumerate() {
            let out = dir.path().join(format!("out{n}"));
            fs::create_dir_all(&out).unwrap();
            let mut seen = Vec::new();
            let report = extract(
                &file,
                &["docs/img".to_string(), "top.txt".to_string()],
                None,
                &out,
                &mut |p| {
                    seen.push(p.to_string());
                    Ok(())
                },
            )
            .unwrap();
            assert_eq!(report.files, 2);
            assert!(report.skipped.is_empty());
            assert_eq!(fs::read_to_string(out.join("img/a.png")).unwrap(), "png");
            assert_eq!(fs::read_to_string(out.join("top.txt")).unwrap(), "top");
            assert!(!out.join("docs").exists());
            seen.sort();
            assert_eq!(seen, ["img/a.png", "top.txt"]);
        }
    }

    #[test]
    fn extracts_everything_under_a_prefix() {
        let dir = TempDir::new().unwrap();
        let zip = dir.path().join("t.zip");
        make_zip(&zip, SAMPLE);
        let out = dir.path().join("out");
        fs::create_dir_all(&out).unwrap();
        let report = extract(&zip, &[String::new()], Some("t"), &out, &mut |_| Ok(())).unwrap();
        assert_eq!(report.files, 3);
        assert!(out.join("t/docs/readme.md").is_file());
        assert!(out.join("t/empty").is_dir());
    }

    #[test]
    fn extraction_can_be_aborted() {
        let dir = TempDir::new().unwrap();
        let zip = dir.path().join("t.zip");
        make_zip(&zip, SAMPLE);
        let out = dir.path().join("out");
        fs::create_dir_all(&out).unwrap();
        let result = extract(&zip, &[String::new()], None, &out, &mut |_| {
            Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"))
        });
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
    }

    #[test]
    fn traversal_names_never_leave_the_destination() {
        let dir = TempDir::new().unwrap();
        let sandbox = dir.path().join("sandbox");
        let out = sandbox.join("out");
        fs::create_dir_all(&out).unwrap();
        let tar = dir.path().join("evil.tar");
        let elsewhere = TempDir::new().unwrap();
        let absolute_path = elsewhere.path().join("absolute.txt");
        let absolute = absolute_path.to_str().unwrap();
        make_tar(
            &tar,
            &[
                Item::File("../escaped.txt", "x"),
                Item::File("a/../../escaped2.txt", "x"),
                Item::File(absolute, "x"),
                Item::File("fine.txt", "ok"),
            ],
            false,
        );
        let report = extract(&tar, &[String::new()], None, &out, &mut |_| Ok(())).unwrap();
        assert_eq!(report.files, 1);
        assert_eq!(report.skipped.len(), 3, "{:?}", report.skipped);
        assert!(out.join("fine.txt").is_file());
        assert!(!sandbox.join("escaped.txt").exists());
        assert!(!dir.path().join("escaped.txt").exists());
        assert!(!dir.path().join("escaped2.txt").exists());
        assert!(!absolute_path.exists());
        // Only the destination was touched.
        let entries: Vec<_> = fs::read_dir(&sandbox).unwrap().flatten().collect();
        assert_eq!(entries.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn links_must_stay_inside() {
        let dir = TempDir::new().unwrap();
        let sandbox = dir.path().join("sandbox");
        let out = sandbox.join("out");
        fs::create_dir_all(&out).unwrap();
        fs::write(sandbox.join("secret"), "s").unwrap();
        for (n, file) in [dir.path().join("l.tar"), dir.path().join("l.zip")]
            .into_iter()
            .enumerate()
        {
            let items = [
                Item::File("data/real.txt", "real"),
                Item::Link("good", "data/real.txt"),
                Item::Link("data/up", "../data/real.txt"),
                Item::Link("abs", "/etc/passwd"),
                Item::Link("up", "../secret"),
                Item::Link("data/deep", "../../secret"),
            ];
            if n == 0 {
                make_tar(&file, &items, false);
            } else {
                make_zip(&file, &items);
            }
            let dest = out.join(n.to_string());
            fs::create_dir_all(&dest).unwrap();
            let report = extract(&file, &[String::new()], None, &dest, &mut |_| Ok(())).unwrap();
            assert_eq!(report.skipped.len(), 3, "{:?}", report.skipped);
            assert!(
                fs::symlink_metadata(dest.join("good"))
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
            assert_eq!(fs::read_to_string(dest.join("data/up")).unwrap(), "real");
            for bad in ["abs", "up", "data/deep"] {
                assert!(fs::symlink_metadata(dest.join(bad)).is_err(), "{bad}");
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn chained_links_cannot_escape() {
        let dir = TempDir::new().unwrap();
        let out = dir.path().join("sandbox/out");
        fs::create_dir_all(&out).unwrap();
        let tar = dir.path().join("c.tar");
        make_tar(
            &tar,
            &[
                Item::Link("l1", "."),
                Item::Link("l2", "l1/.."),
                Item::File("ok.txt", "ok"),
            ],
            false,
        );
        let report = extract(&tar, &[String::new()], None, &out, &mut |_| Ok(())).unwrap();
        assert_eq!(report.files, 2, "{report:?}");
        assert_eq!(report.skipped.len(), 1, "{:?}", report.skipped);
        assert!(fs::symlink_metadata(out.join("l1")).is_ok());
        assert!(fs::symlink_metadata(out.join("l2")).is_err());
    }

    #[test]
    fn unreadable_zip_entry_does_not_break_the_rest() {
        let dir = TempDir::new().unwrap();
        let zip = dir.path().join("u.zip");
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        let mut writer = zip::ZipWriter::new(File::create(&zip).unwrap());
        writer.start_file("bad.txt", opts).unwrap();
        writer.write_all(b"bad").unwrap();
        writer.start_file("good.txt", opts).unwrap();
        writer.write_all(b"good").unwrap();
        writer.finish().unwrap();
        // Flag the first entry as bzip2, which is not built in.
        let mut bytes = fs::read(&zip).unwrap();
        let local = bytes.windows(4).position(|w| w == b"PK\x03\x04").unwrap();
        let central = bytes.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
        bytes[local + 8] = 12;
        bytes[central + 10] = 12;
        fs::write(&zip, bytes).unwrap();

        let mut out = Vec::new();
        read_file(&zip, "good.txt", 100, &mut out).unwrap();
        assert_eq!(out, b"good");
        assert!(read_file(&zip, "bad.txt", 100, &mut Vec::new()).is_err());

        let dest = dir.path().join("out");
        fs::create_dir_all(&dest).unwrap();
        let report = extract(&zip, &[String::new()], None, &dest, &mut |_| Ok(())).unwrap();
        assert_eq!(report.files, 1);
        assert_eq!(report.skipped.len(), 1, "{:?}", report.skipped);
        assert_eq!(fs::read_to_string(dest.join("good.txt")).unwrap(), "good");
    }

    #[cfg(unix)]
    #[test]
    fn nothing_is_written_through_a_link() {
        // A link to a directory inside the destination is allowed, but a
        // later entry must not be written through it.
        let dir = TempDir::new().unwrap();
        let out = dir.path().join("out");
        fs::create_dir_all(out.join("real")).unwrap();
        let tar = dir.path().join("t.tar");
        make_tar(
            &tar,
            &[Item::Link("l", "real"), Item::File("l/x.txt", "x")],
            false,
        );
        let result = extract(&tar, &[String::new()], None, &out, &mut |_| Ok(()));
        assert!(result.is_err());
        assert!(!out.join("real/x.txt").exists());
    }

    #[test]
    fn link_rules() {
        assert!(link_stays_inside("a", "b"));
        assert!(link_stays_inside("a/b", "../c"));
        assert!(link_stays_inside("a/b", "./c/../d"));
        assert!(!link_stays_inside("a", "../b"));
        assert!(!link_stays_inside("a/b", "../../c"));
        assert!(!link_stays_inside("a", "/etc"));
        assert!(!link_stays_inside("a", ""));
        assert!(!link_stays_inside("a", "C:\\x"));
    }

    #[test]
    fn relative_destinations() {
        assert_eq!(relative_to("a/b/c", "a/b", None), Some("b/c".into()));
        assert_eq!(relative_to("a/b", "a/b", None), Some("b".into()));
        assert_eq!(relative_to("a/bc", "a/b", None), None);
        assert_eq!(relative_to("a/b", "", Some("z")), Some("z/a/b".into()));
        assert_eq!(relative_to("a", "a", None), Some("a".into()));
    }

    #[test]
    fn locations_and_extracted_entries_name_the_archive_path() {
        assert_eq!(location(Path::new("/a/f.zip"), "x/y"), "/a/f.zip › x/y");
        assert_eq!(location(Path::new("/a/f.zip"), ""), "/a/f.zip ›");
        let wanted = ["d/x".to_string()];
        assert_eq!(entry_of("x/f", &wanted, None).as_deref(), Some("d/x/f"));
        assert_eq!(
            entry_of("f/x/f", &wanted, Some("f")).as_deref(),
            Some("d/x/f")
        );
        assert_eq!(entry_of("y", &wanted, None), None);
        assert_eq!(
            entry_of("a/b", &[String::new()], None).as_deref(),
            Some("a/b")
        );
    }
}
