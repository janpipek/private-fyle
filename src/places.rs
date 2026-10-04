//! Places to jump to, UI-independent: the user's bookmarks, the desktop's
//! standard and favourite locations, and frequently visited directories
//! ranked zoxide-style ("frecency").

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub label: String,
    pub path: PathBuf,
}

impl Place {
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
    entries: HashMap<PathBuf, (u32, u64)>,
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

    /// Lines of `count<TAB>unix-seconds<TAB>path`.
    pub fn parse(text: &str) -> Self {
        let entries = text
            .lines()
            .filter_map(|line| {
                let mut parts = line.splitn(3, '\t');
                let count = parts.next()?.parse().ok()?;
                let time = parts.next()?.parse().ok()?;
                Some((PathBuf::from(parts.next()?), (count, time)))
            })
            .collect();
        Self { entries }
    }

    pub fn serialize(&self) -> String {
        let mut lines: Vec<_> = self
            .entries
            .iter()
            .map(|(path, (count, time))| format!("{count}\t{time}\t{}\n", path.display()))
            .collect();
        lines.sort();
        lines.concat()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, self.serialize())
    }

    pub fn visit(&mut self, dir: &Path, now: u64) {
        let entry = self.entries.entry(dir.to_path_buf()).or_insert((0, now));
        entry.0 = entry.0.saturating_add(1);
        entry.1 = now;
        if self.entries.len() > MAX_HISTORY {
            // Forget the lowest-ranked directories.
            let mut ranked = self.ranked(now);
            ranked.truncate(MAX_HISTORY * 9 / 10);
            self.entries.retain(|path, _| ranked.contains(path));
        }
    }

    /// Paths by frecency: visit count weighted by recency, as zoxide does
    /// (×4 within an hour, ×2 within a day, ×½ within a week, else ×¼).
    pub fn ranked(&self, now: u64) -> Vec<PathBuf> {
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
                .then_with(|| a.0.cmp(b.0))
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
            ranked,
            [PathBuf::from("/recent"), "/today".into(), "/old".into()]
        );

        let restored = History::parse(&history.serialize());
        assert_eq!(restored, history);
        assert_eq!(History::parse("garbage\n1\tx\t/p\n"), History::default());
    }
}
