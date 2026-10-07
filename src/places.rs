//! Places to jump to, UI-independent: the user's bookmarks, the desktop's
//! standard and favourite locations, and frequently visited directories
//! ranked zoxide-style ("frecency").

pub mod history_save;

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::location::Location;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub label: String,
    pub path: PathBuf,
}

/// A discoverable section in the panel locations picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// What the group holds; views pick its icon from this.
    pub kind: GroupKind,
    /// Section heading in the locations picker.
    pub label: String,
    /// Locations in this section.
    pub places: Vec<Place>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupKind {
    Volumes,
    Places,
    Bookmarks,
    Frequent,
    /// Remote services (`<name>:/`), never checked against the disk.
    Remotes,
}

/// Groups longer than this show as a submenu in the locations menu.
pub const INLINE_LIMIT: usize = 6;

impl Group {
    /// Whether the locations menu nests this group in a submenu.
    pub fn as_submenu(&self) -> bool {
        self.places.len() > INLINE_LIMIT
    }
}

impl Place {
    /// A configured remote's root: the label shown and `<name>:/`.
    pub fn remote(name: &str, label: &str) -> Self {
        Self::new(label, format!("{name}:/"))
    }

    fn new(label: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            label: label.into(),
            path: path.into(),
        }
    }
}

/// Home, the XDG / known user folders, desktop bookmarks (GTK, KDE) and
/// drives on Windows; only existing directories, without duplicates.
pub fn system_places() -> Vec<Place> {
    let mut places = Vec::new();
    if let Some(home) = dirs::home_dir() {
        places.push(Place::new("Home", home));
    }
    for (label, dir) in [
        ("Desktop", dirs::desktop_dir()),
        ("Documents", dirs::document_dir()),
        ("Downloads", dirs::download_dir()),
        ("Pictures", dirs::picture_dir()),
        ("Music", dirs::audio_dir()),
        ("Videos", dirs::video_dir()),
    ] {
        if let Some(dir) = dir {
            places.push(Place::new(label, dir));
        }
    }
    if let Some(config) = dirs::config_dir() {
        for file in [
            config.join("gtk-3.0/bookmarks"),
            config.join("gtk-4.0/bookmarks"),
        ] {
            if let Ok(text) = std::fs::read_to_string(file) {
                places.extend(parse_gtk_bookmarks(&text));
            }
        }
    }
    if let Some(data) = dirs::data_dir()
        && let Ok(text) = std::fs::read_to_string(data.join("user-places.xbel"))
    {
        places.extend(parse_kde_places(&text));
    }
    #[cfg(windows)]
    for letter in b'A'..=b'Z' {
        let root = PathBuf::from(format!("{}:\\", letter as char));
        places.push(Place::new(root.display().to_string(), root));
    }
    #[cfg(not(windows))]
    places.push(Place::new("Root", "/"));
    dedup_existing(places)
}

/// Drives and mounted volumes, desktop places, remotes, then fyle bookmarks.
/// Discovery reads desktop configuration and mount tables, so callers run it
/// off the UI thread. `remotes` come from [`Place::remote`] and are listed
/// as given, without touching the disk.
#[allow(dead_code)] // Kept as the panel-path adapter for older callers.
pub fn grouped_locations(
    bookmarks: &[PathBuf],
    frequent: &[PathBuf],
    remotes: Vec<Place>,
) -> Vec<Group> {
    let frequent: Vec<_> = frequent
        .iter()
        .filter_map(|path| Location::from_panel_path(path, None).ok())
        .collect();
    grouped_locations_typed(bookmarks, &frequent, remotes)
}

/// Groups places using typed frequent locations so virtual addresses keep
/// their identity through history and the locations picker.
pub fn grouped_locations_typed(
    bookmarks: &[PathBuf],
    frequent: &[Location],
    remotes: Vec<Place>,
) -> Vec<Group> {
    let group = |kind: GroupKind,
                 label: &str,
                 places: Vec<Place>,
                 virtual_paths: &std::collections::HashSet<PathBuf>| {
        let mut seen = std::collections::HashSet::new();
        let places = places
            .into_iter()
            // Remote paths are never checked on the disk; one that is gone
            // fails when opened.
            .filter(|place| {
                (virtual_paths.contains(&place.path) || place.path.is_dir())
                    && seen.insert(place.path.clone())
            })
            .collect();
        Group {
            kind,
            label: label.into(),
            places,
        }
    };
    let no_virtual_paths = std::collections::HashSet::new();
    let drives = group(
        GroupKind::Volumes,
        "Drives & Volumes",
        mounted_volumes(),
        &no_virtual_paths,
    );
    let places = group(
        GroupKind::Places,
        "Places",
        system_places(),
        &no_virtual_paths,
    );
    let bookmarks = group(
        GroupKind::Bookmarks,
        "Bookmarks",
        bookmarks
            .iter()
            .map(|path| Place::new(crate::tools::file_name(path), path.clone()))
            .collect(),
        &no_virtual_paths,
    );
    let virtual_paths: std::collections::HashSet<_> = frequent
        .iter()
        .filter(|location| matches!(location, Location::Remote(_)))
        .map(Location::panel_path)
        .collect();
    let frequent = group(
        GroupKind::Frequent,
        "Frequent",
        frequent
            .iter()
            .map(|location| {
                let path = location.panel_path();
                let label = if matches!(location, Location::Remote(_)) {
                    path.display().to_string()
                } else {
                    crate::tools::file_name(&path)
                };
                Place::new(label, path)
            })
            .collect(),
        &virtual_paths,
    );
    let remotes = Group {
        kind: GroupKind::Remotes,
        label: "Remotes".into(),
        places: remotes,
    };
    [drives, places, remotes, bookmarks, frequent]
        .into_iter()
        .filter(|group| !group.places.is_empty())
        .collect()
}

#[cfg(windows)]
fn mounted_volumes() -> Vec<Place> {
    system_places()
        .into_iter()
        .filter(|place| place.path.parent().is_none())
        .collect()
}

#[cfg(not(windows))]
fn mounted_volumes() -> Vec<Place> {
    let text = std::fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
    parse_mountinfo(&text)
}

/// Parses Linux mountinfo into local volumes, excluding virtual kernel file
/// systems. Kept public so parsing can be tested without reading the host.
#[cfg(not(windows))]
pub fn parse_mountinfo(text: &str) -> Vec<Place> {
    let mut places = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in text.lines() {
        let Some((before, after)) = line.split_once(" - ") else {
            continue;
        };
        let fields: Vec<_> = before.split_whitespace().collect();
        let Some(raw_path) = fields.get(4) else {
            continue;
        };
        let Some(fs_type) = after.split_whitespace().next() else {
            continue;
        };
        if is_virtual_filesystem(fs_type) {
            continue;
        }
        let path = PathBuf::from(unescape_mount_field(raw_path));
        if is_user_mount(&path, fs_type) && seen.insert(path.clone()) {
            let label = if path == Path::new("/") {
                "Root".into()
            } else {
                crate::tools::file_name(&path)
            };
            places.push(Place::new(label, path));
        }
    }
    places
}

#[cfg(not(windows))]
fn unescape_mount_field(field: &str) -> String {
    field
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}

/// Whether a mount is one people browse: the root, home, removable media and
/// manual mounts, and network shares anywhere. Hides snaps, boot partitions,
/// container layers and other system mounts.
#[cfg(not(windows))]
fn is_user_mount(path: &Path, fs_type: &str) -> bool {
    let network = fs_type.starts_with("nfs")
        || fs_type.starts_with("smb")
        || matches!(
            fs_type,
            "cifs" | "9p" | "davfs" | "fuse.sshfs" | "fuse.rclone"
        );
    let user_dir = ["/home", "/media", "/run/media", "/mnt"]
        .iter()
        .any(|dir| path.starts_with(dir));
    fs_type != "squashfs" && (path == Path::new("/") || user_dir || network)
}

#[cfg(not(windows))]
fn is_virtual_filesystem(fs_type: &str) -> bool {
    matches!(
        fs_type,
        "autofs"
            | "bpf"
            | "cgroup"
            | "cgroup2"
            | "configfs"
            | "debugfs"
            | "devpts"
            | "devtmpfs"
            | "fusectl"
            | "hugetlbfs"
            | "mqueue"
            | "proc"
            | "pstore"
            | "securityfs"
            | "sysfs"
            | "tmpfs"
            | "tracefs"
    )
}

fn dedup_existing(places: Vec<Place>) -> Vec<Place> {
    let mut seen = std::collections::HashSet::new();
    places
        .into_iter()
        .filter(|p| p.path.is_dir() && seen.insert(p.path.clone()))
        .collect()
}

/// `~/.config/gtk-3.0/bookmarks`: lines of `file:///path [label]`.
pub fn parse_gtk_bookmarks(text: &str) -> Vec<Place> {
    text.lines()
        .filter_map(|line| {
            let (url, label) = line.split_once(' ').unwrap_or((line, ""));
            let path = file_url_to_path(url.trim())?;
            let label = if label.trim().is_empty() {
                crate::tools::file_name(&path)
            } else {
                label.trim().to_string()
            };
            Some(Place::new(label, path))
        })
        .collect()
}

/// KDE's `user-places.xbel`: `<bookmark href="file:///…"><title>…</title>`.
/// A light scan rather than a full XML parser.
pub fn parse_kde_places(xml: &str) -> Vec<Place> {
    let mut places = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<bookmark ") {
        rest = &rest[start..];
        let end = rest.find("</bookmark>").unwrap_or(rest.len());
        let block = &rest[..end];
        rest = &rest[end..];
        let href = block
            .split_once("href=\"")
            .and_then(|(_, r)| r.split_once('"'))
            .map(|(href, _)| href);
        let Some(path) = href.and_then(|h| file_url_to_path(&unescape_xml(h))) else {
            continue;
        };
        let title = block
            .split_once("<title>")
            .and_then(|(_, r)| r.split_once("</title>"))
            .map(|(t, _)| unescape_xml(t.trim()))
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| crate::tools::file_name(&path));
        places.push(Place::new(title, path));
    }
    places
}

fn unescape_xml(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// `file:///a%20b` → `/a b`; other schemes → `None`.
pub fn file_url_to_path(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Ok(byte) = u8::from_str_radix(rest.get(i + 1..i + 3).unwrap_or(""), 16)
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    let path = String::from_utf8(out).ok()?;
    // file:///C:/x on Windows.
    let path = match path.strip_prefix('/') {
        Some(p) if p.as_bytes().get(1) == Some(&b':') => p.to_string(),
        _ => path,
    };
    Some(PathBuf::from(path))
}

/// Visit counts and times per directory.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct History {
    entries: HashMap<Location, (u32, u64)>,
}

/// Directories kept in the history.
const MAX_HISTORY: usize = 1000;

impl History {
    /// `<data dir>/fyle/history.tsv`.
    pub fn path() -> Option<PathBuf> {
        dirs::data_dir().map(|d| d.join("fyle").join("history.tsv"))
    }

    pub fn load(path: &Path) -> Self {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        Self::parse(&text)
    }

    /// Parses typed entries and the legacy `count<TAB>unix-seconds<TAB>path`
    /// format. Invalid entries are ignored so one bad line cannot lose history.
    pub fn parse(text: &str) -> Self {
        let entries = text
            .lines()
            .filter_map(|line| {
                let mut parts = line.splitn(3, '\t');
                let count = parts.next()?.parse().ok()?;
                let time = parts.next()?.parse().ok()?;
                let value = parts.next()?;
                let location = if value.starts_with(crate::location_codec::PREFIX) {
                    crate::location_codec::decode(value).ok()?
                } else {
                    Location::from_panel_path(Path::new(value), None).ok()?
                };
                Some((location, (count, time)))
            })
            .collect();
        Self { entries }
    }

    /// Serializes each location as one versioned TSV field.
    pub fn serialize(&self) -> String {
        let mut lines: Vec<_> = self
            .entries
            .iter()
            .filter_map(|(location, (count, time))| {
                crate::location_codec::encode(location)
                    .ok()
                    .map(|encoded| format!("{count}\t{time}\t{encoded}\n"))
            })
            .collect();
        lines.sort();
        lines.concat()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        let contents = self.serialize();
        static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
        let parent = path
            .parent()
            .filter(|dir| !dir.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let name = path
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("history"));
        let mut last_collision = None;
        for _ in 0..128 {
            let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let mut temp_name = std::ffi::OsString::from(".");
            temp_name.push(name);
            temp_name.push(format!(".{}.{}.tmp", std::process::id(), sequence));
            let temp = parent.join(temp_name);
            let file = match OpenOptions::new().write(true).create_new(true).open(&temp) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    last_collision = Some(error);
                    continue;
                }
                Err(error) => return Err(error),
            };
            let result = (|| {
                let mut file = file;
                file.write_all(contents.as_bytes())?;
                file.sync_all()?;
                drop(file);
                std::fs::rename(&temp, path)
            })();
            if result.is_err() {
                let _ = std::fs::remove_file(&temp);
            }
            return result;
        }
        Err(last_collision.unwrap_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "could not reserve history temp file",
            )
        }))
    }

    /// Adds a visit for a typed address.
    pub fn visit_location(&mut self, location: Location, now: u64) {
        let entry = self.entries.entry(location).or_insert((0, now));
        entry.0 = entry.0.saturating_add(1);
        entry.1 = now;
        if self.entries.len() > MAX_HISTORY {
            let mut ranked = self.ranked(now);
            ranked.truncate(MAX_HISTORY * 9 / 10);
            self.entries.retain(|location, _| ranked.contains(location));
        }
    }

    /// Compatibility adapter for callers that still have a panel path.
    #[allow(dead_code)] // Keeps callers outside this migration able to record legacy paths.
    pub fn visit(&mut self, dir: &Path, now: u64) {
        if let Ok(location) = Location::from_panel_path(dir, None) {
            self.visit_location(location, now);
        }
    }

    /// Paths by frecency: visit count weighted by recency, as zoxide does
    /// (×4 within an hour, ×2 within a day, ×½ within a week, else ×¼).
    pub fn ranked(&self, now: u64) -> Vec<Location> {
        let score = |(count, time): (u32, u64)| {
            let age = now.saturating_sub(time);
            let weight = match age {
                0..3600 => 4.,
                3600..86_400 => 2.,
                86_400..604_800 => 0.5,
                _ => 0.25,
            };
            count as f64 * weight
        };
        let mut paths: Vec<_> = self.entries.iter().collect();
        paths.sort_by(|a, b| {
            score(*b.1)
                .total_cmp(&score(*a.1))
                .then_with(|| a.0.panel_path().cmp(&b.0.panel_path()))
        });
        paths.into_iter().map(|(p, _)| p.clone()).collect()
    }
}

/// `path` with the home directory shown as `~`.
pub fn tilde(path: &Path) -> String {
    match dirs::home_dir().and_then(|home| path.strip_prefix(&home).ok().map(Path::to_path_buf)) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        None => path.display().to_string(),
    }
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frequent_keeps_remote_folders_without_disk_checks() {
        let mut history = History::default();
        history.visit(Path::new("gdrive:/Docs/2026"), 100);
        history.visit(Path::new("gdrive:/Docs/2026"), 200);
        let frequent = history.ranked(300);
        let groups = grouped_locations_typed(&[], &frequent, Vec::new());
        let group = groups
            .iter()
            .find(|g| g.kind == GroupKind::Frequent)
            .expect("frequent group");
        assert_eq!(group.places.len(), 1);
        assert_eq!(group.places[0].path, Path::new("gdrive:/Docs/2026"));
        assert_eq!(group.places[0].label, "gdrive:/Docs/2026");
        let text = history.serialize();
        assert_eq!(History::parse(&text), history);
    }

    #[test]
    fn remotes_group_lists_remotes_without_disk_checks() {
        let remotes = vec![
            Place::remote("proton", "Proton Drive"),
            Place::remote("gdrive", "Google Drive"),
        ];
        let groups = grouped_locations(&[], &[], remotes.clone());
        let group = groups
            .iter()
            .find(|g| g.kind == GroupKind::Remotes)
            .expect("remotes group");
        assert_eq!(group.label, "Remotes");
        assert_eq!(group.places, remotes);
        assert_eq!(group.places[0].path, Path::new("proton:/"));
        let none = grouped_locations(&[], &[], Vec::new());
        assert!(none.iter().all(|g| g.kind != GroupKind::Remotes));
    }

    #[test]
    fn parses_gtk_bookmarks() {
        let places = parse_gtk_bookmarks(
            "file:///home/u/My%20Projects Projects\nfile:///srv/data\nsftp://host/x Remote\n",
        );
        assert_eq!(
            places,
            [
                Place::new("Projects", "/home/u/My Projects"),
                Place::new("data", "/srv/data"),
            ]
        );
    }

    #[test]
    fn parses_kde_places() {
        let xml = r#"<?xml version="1.0"?><xbel>
 <bookmark href="file:///home/u">
  <title>Home</title><info>…</info>
 </bookmark>
 <bookmark href="file:///mnt/a%26b"><title>A &amp; B</title></bookmark>
 <bookmark href="remote:/"><title>Network</title></bookmark>
 <bookmark href="file:///opt/x"></bookmark>
</xbel>"#;
        assert_eq!(
            parse_kde_places(xml),
            [
                Place::new("Home", "/home/u"),
                Place::new("A & B", "/mnt/a&b"),
                Place::new("x", "/opt/x"),
            ]
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn parses_real_mounts_and_skips_kernel_filesystems() {
        let places = parse_mountinfo(
            "24 20 8:1 / / rw,relatime - ext4 /dev/sda1 rw\n\
             25 24 0:22 / /proc rw,nosuid - proc proc rw\n\
             31 24 8:2 / /media/My\\040Disk rw - vfat /dev/sdb1 rw\n",
        );
        assert_eq!(
            places,
            [
                Place::new("Root", "/"),
                Place::new("My Disk", "/media/My Disk"),
            ]
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn hides_snaps_and_system_mounts() {
        let places = parse_mountinfo(
            "24 20 8:1 / / rw - ext4 /dev/sda1 rw\n\
             26 24 8:3 / /home rw - btrfs /dev/sda3 rw\n\
             27 24 8:4 / /boot rw - ext4 /dev/sda4 rw\n\
             28 24 8:5 / /boot/efi rw - vfat /dev/sda5 rw\n\
             29 24 7:0 / /snap/core22/1380 ro - squashfs /dev/loop0 ro\n\
             30 24 7:1 / /media/u/snapdisk ro - squashfs /dev/loop1 ro\n\
             32 24 0:40 / /var/lib/docker/overlay2/x/merged rw - overlay overlay rw\n\
             33 24 0:41 / /srv/share rw - nfs4 nas:/share rw\n\
             34 24 8:17 / /run/media/u/USB rw - exfat /dev/sdb1 rw\n\
             35 24 8:18 / /mnt/backup rw - ext4 /dev/sdc1 rw\n",
        );
        let paths: Vec<_> = places.iter().map(|p| p.path.to_str().unwrap()).collect();
        assert_eq!(
            paths,
            [
                "/",
                "/home",
                "/srv/share",
                "/run/media/u/USB",
                "/mnt/backup"
            ]
        );
    }

    #[test]
    fn long_groups_become_submenus() {
        let group = |n| Group {
            kind: GroupKind::Frequent,
            label: "G".into(),
            places: (0..n).map(|i| Place::new("p", format!("/p{i}"))).collect(),
        };
        assert!(!group(INLINE_LIMIT).as_submenu());
        assert!(group(INLINE_LIMIT + 1).as_submenu());
    }

    #[test]
    fn file_urls() {
        assert_eq!(
            file_url_to_path("file:///a%20b/%C3%A9"),
            Some("/a b/é".into())
        );
        assert_eq!(
            file_url_to_path("file:///C:/Users"),
            Some("C:/Users".into())
        );
        assert_eq!(file_url_to_path("file:///bad%2"), Some("/bad%2".into()));
        assert_eq!(file_url_to_path("https://x"), None);
    }

    #[test]
    fn frecency_ranking_and_roundtrip() {
        let now = 10_000_000;
        let mut history = History::default();
        for _ in 0..3 {
            history.visit(Path::new("/old"), now - 30 * 86_400); // 3 × ¼
        }
        history.visit(Path::new("/recent"), now - 60); // 1 × 4
        history.visit(Path::new("/today"), now - 7200);
        history.visit(Path::new("/today"), now - 7200); // 2 × 2
        let ranked = history.ranked(now);
        assert_eq!(
            ranked.iter().map(Location::panel_path).collect::<Vec<_>>(),
            [PathBuf::from("/recent"), "/today".into(), "/old".into()]
        );

        let restored = History::parse(&history.serialize());
        assert_eq!(restored, history);
        assert_eq!(History::parse("garbage\n1\tx\t/p\n"), History::default());
    }

    #[test]
    fn history_migrates_legacy_paths_and_keeps_literal_local_colons_distinct() {
        let legacy = History::parse("2\t100\t/mnt/nas:/x\n3\t200\tgdrive:/Docs\n");
        assert_eq!(
            legacy.ranked(300)[0].panel_path(),
            PathBuf::from("gdrive:/Docs")
        );
        assert!(matches!(&legacy.ranked(300)[1], Location::Local(_)));

        let literal = Location::local("/mnt/nas:/x".into()).unwrap();
        let mut history = History::default();
        history.visit_location(literal.clone(), 10);
        let saved = history.serialize();
        assert!(saved.contains(crate::location_codec::PREFIX));
        assert_eq!(History::parse(&saved).ranked(10), [literal]);
    }

    #[test]
    fn history_save_replaces_existing_file_and_roundtrips_control_paths() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.tsv");
        std::fs::write(&path, "old data").unwrap();
        let mut history = History::default();
        #[cfg(unix)]
        {
            use std::ffi::OsString;
            use std::os::unix::ffi::OsStringExt;
            let native =
                PathBuf::from("/tmp").join(OsString::from_vec(vec![b'a', 0xff, b'\t', b'\n']));
            history.visit_location(Location::local(native).unwrap(), 42);
        }
        history.save(&path).unwrap();
        assert_eq!(History::load(&path), history);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
