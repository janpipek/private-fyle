//! Command palette model: every command with its label and key bindings,
//! filtered by typed words. UI-independent; actions are opaque here.

/// True if every whitespace-separated word of `query` occurs in `text`
/// (case-insensitive). An empty query matches everything.
pub fn words_match(text: &str, query: &str) -> bool {
    let text = text.to_lowercase();
    query
        .split_whitespace()
        .all(|word| text.contains(&word.to_lowercase()))
}

/// A palette entry. `T` is whatever the UI runs (an action).
pub struct Command<T> {
    pub label: String,
    /// Key bindings for display, e.g. `"f5"`; may be empty.
    pub keys: String,
    pub payload: T,
}

pub struct Palette<T> {
    commands: Vec<Command<T>>,
    matches: Vec<usize>,
}

impl<T> Palette<T> {
    pub fn new(commands: Vec<Command<T>>) -> Self {
        Self {
            matches: (0..commands.len()).collect(),
            commands,
        }
    }

    /// Keeps commands whose label or keys contain every word of `query`.
    pub fn filter(&mut self, query: &str) {
        self.matches = (0..self.commands.len())
            .filter(|&i| {
                let c = &self.commands[i];
                words_match(&format!("{} {}", c.label, c.keys), query)
            })
            .collect();
    }

    pub fn len(&self) -> usize {
        self.matches.len()
    }

    pub fn get(&self, ix: usize) -> Option<&Command<T>> {
        self.matches.get(ix).map(|&i| &self.commands[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> Palette<u8> {
        let cmd = |label: &str, keys: &str, payload| Command {
            label: label.into(),
            keys: keys.into(),
            payload,
        };
        Palette::new(vec![
            cmd("Copy to other panel", "f5", 1),
            cmd("Move to trash", "f8, delete", 2),
            cmd("Move to other panel", "f6", 3),
        ])
    }

    #[test]
    fn filters_by_words_in_label_and_keys() {
        let mut p = palette();
        assert_eq!(p.len(), 3);
        p.filter("move PANEL");
        assert_eq!(p.len(), 1);
        assert_eq!(p.get(0).unwrap().payload, 3);
        p.filter("f8");
        assert_eq!(p.get(0).unwrap().payload, 2);
        p.filter("nothing");
        assert_eq!(p.len(), 0);
        assert!(p.get(0).is_none());
        p.filter("");
        assert_eq!(p.len(), 3);
    }

    #[test]
    fn word_matching() {
        assert!(words_match("Tokyo Night", "night tok"));
        assert!(!words_match("Tokyo Night", "storm"));
        assert!(words_match("x", "  "));
    }
}
