//! A minimal reader for the INI-style files of the freedesktop specs
//! (`.desktop` files, `mimeapps.list`); UI-independent.

/// Sections in file order; keys keep their order and duplicates.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Ini {
    sections: Vec<(String, Vec<(String, String)>)>,
}

impl Ini {
    /// Parses `text`: `[Section]` headers, `key=value` lines, `#` comments.
    /// Lines before the first header are ignored; a repeated section header
    /// continues the earlier section.
    pub fn parse(text: &str) -> Self {
        let mut ini = Self::default();
        let mut current: Option<usize> = None;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                current = Some(match ini.sections.iter().position(|(n, _)| n == name) {
                    Some(ix) => ix,
                    None => {
                        ini.sections.push((name.to_string(), Vec::new()));
                        ini.sections.len() - 1
                    }
                });
            } else if let (Some(ix), Some((key, value))) = (current, line.split_once('=')) {
                ini.sections[ix]
                    .1
                    .push((key.trim().to_string(), value.trim().to_string()));
            }
        }
        ini
    }

    /// The entries of `section`, empty if it does not exist.
    pub fn section(&self, section: &str) -> &[(String, String)] {
        self.sections
            .iter()
            .find(|(n, _)| n == section)
            .map_or(&[], |(_, entries)| entries.as_slice())
    }

    /// The first value of `key` in `section`.
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.section(section)
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::Ini;

    #[test]
    fn parses_sections_comments_and_repeats() {
        let ini =
            Ini::parse("ignored=1\n# c\n[A]\nk = v \nk=w\n\n[B]\nx=y=z\n[A]\nlate=1\n[Empty]\n");
        assert_eq!(ini.get("A", "k"), Some("v"));
        assert_eq!(ini.get("A", "late"), Some("1"));
        assert_eq!(ini.get("B", "x"), Some("y=z"));
        assert_eq!(ini.section("A").len(), 3);
        assert!(ini.section("Empty").is_empty());
        assert_eq!(ini.get("nope", "k"), None);
    }
}
