//! Back and Forward history of visited directories.

use std::path::PathBuf;

use super::Folder;

/// A directory in the Back/Forward history and its cursor entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Visit {
    path: PathBuf,
    selected: Option<PathBuf>,
}

/// History entries kept per direction.
const HISTORY_LEN: usize = 100;

fn push_capped(list: &mut Vec<Visit>, visit: Visit) {
    if list.last() != Some(&visit) {
        list.push(visit);
    }
    if list.len() > HISTORY_LEN {
        list.remove(0);
    }
}

impl Folder {
    /// Where Back (or Forward) would go.
    pub fn history_target(&self, back: bool) -> Option<PathBuf> {
        let list = if back { &self.back } else { &self.forward };
        list.last().map(|v| v.path.clone())
    }

    /// Shows `path`, recording the directory left in the Back history.
    pub fn replace(&mut self, path: PathBuf, select: Option<PathBuf>) -> Result<(), String> {
        let left = self.visit();
        self.replace_untracked(path, select)?;
        if let Some(visit) = left.filter(|v| v.path != self.path) {
            push_capped(&mut self.back, visit);
            self.forward.clear();
        }
        Ok(())
    }

    /// The directory shown now as a history entry (none for search results).
    pub fn visit(&self) -> Option<Visit> {
        (self.results.is_none()).then(|| Visit {
            path: self.path.clone(),
            selected: self.current().map(|e| e.path.clone()),
        })
    }

    /// Back: the previous directory, cursor where it was. Returns whether
    /// there was one.
    pub fn go_back(&mut self) -> Result<bool, String> {
        self.travel(true)
    }

    pub fn go_forward(&mut self) -> Result<bool, String> {
        self.travel(false)
    }

    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    /// One step back (or forward): the current directory goes onto the
    /// other list. A directory that is gone is dropped from the history.
    pub fn travel(&mut self, back: bool) -> Result<bool, String> {
        let from = if back {
            &mut self.back
        } else {
            &mut self.forward
        };
        let Some(target) = from.pop() else {
            return Ok(false);
        };
        let here = self.visit();
        self.replace_untracked(target.path, target.selected)?;
        if let Some(here) = here {
            push_capped(
                if back {
                    &mut self.forward
                } else {
                    &mut self.back
                },
                here,
            );
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {

    use crate::model::test_support::{fixture, loaded};

    use std::fs;

    #[test]
    fn back_and_forward() {
        let dir = fixture();
        fs::create_dir(dir.path().join("sub").join("deep")).unwrap();
        let mut folder = loaded(&dir);
        let sub = dir.path().join("sub");
        assert!(!folder.can_go_back());
        assert_eq!(folder.go_back(), Ok(false));

        folder.set_cursor(2); // a.txt
        folder.navigate(sub.clone()).unwrap();
        folder.navigate(sub.join("deep")).unwrap();
        folder.load().unwrap(); // a reload is no visit
        assert_eq!(folder.go_back(), Ok(true));
        assert_eq!(folder.path(), sub);
        assert_eq!(folder.go_back(), Ok(true));
        assert_eq!(folder.path(), dir.path());
        assert_eq!(folder.current().unwrap().name, "a.txt"); // cursor restored
        assert!(!folder.can_go_back());

        assert_eq!(folder.go_forward(), Ok(true));
        assert_eq!(folder.path(), sub);
        // A new visit clears Forward.
        folder.go_up().unwrap();
        assert!(!folder.can_go_forward());
        assert_eq!(folder.go_back(), Ok(true));
        assert_eq!(folder.path(), sub);

        // A vanished directory is dropped with an error.
        folder.navigate(sub.join("deep")).unwrap();
        folder.go_back().unwrap();
        fs::remove_dir(sub.join("deep")).unwrap();
        assert!(folder.go_forward().is_err());
        assert!(!folder.can_go_forward());
        assert_eq!(folder.path(), sub);
    }

    #[test]
    fn history_target_names_the_next_directory() {
        let dir = fixture();
        let mut folder = loaded(&dir);
        assert_eq!(folder.history_target(true), None);
        let sub = folder.current().unwrap().path.clone();
        assert_eq!(folder.activation_target(), Some(sub.clone()));
        folder.activate().unwrap();
        assert_eq!(folder.history_target(true).as_deref(), Some(dir.path()));
        folder.go_back().unwrap();
        assert_eq!(folder.history_target(false), Some(sub));
    }
}
