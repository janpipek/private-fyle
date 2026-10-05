//! Commands as the user sees them: names, where they appear in the menu
//! bar, toggle states and availability. UI-independent; the menu bar and
//! the command palette (and later the context menu) are built from this,
//! so names and availability agree everywhere. `commander.rs` maps each
//! [`Cmd`] to its gpui action.

/// A user-visible command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cmd {
    Open,
    OpenWith,
    EditFile,
    NewFile,
    MakeDir,
    Rename,
    Copy,
    Move,
    Delete,
    DeletePermanently,
    Quit,
    CutFiles,
    YankFiles,
    PasteFiles,
    ToggleMark,
    SelectAll,
    InvertSelection,
    SelectPattern,
    Unmark,
    Filter,
    TogglePreview,
    ToggleTree,
    Expand,
    Collapse,
    ToggleHidden,
    SelectColumns,
    SelectTheme,
    Refresh,
    GoUp,
    GoFirst,
    GoLast,
    SwitchPanel,
    GoTo,
    AddBookmark,
    OpenTerminal,
    ToolsMenu,
    FindFiles,
    CancelJobs,
    CommandPalette,
    Help,
}

/// What availability and toggle states depend on, taken from the window.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    /// Something to act on: marked entries, or a cursor entry other than "..".
    pub has_targets: bool,
    /// The cursor is on a regular file (not a directory).
    pub cursor_on_file: bool,
    /// Entries were yanked or cut and can be pasted.
    pub can_paste: bool,
    /// The panel lists a directory (not search results or the drive list).
    pub in_directory: bool,
    pub tree: bool,
    pub hidden: bool,
    pub preview: bool,
    pub jobs_running: bool,
}

impl Cmd {
    /// Every command, in palette order.
    pub const ALL: [Cmd; 40] = [
        Cmd::Open,
        Cmd::OpenWith,
        Cmd::GoUp,
        Cmd::GoFirst,
        Cmd::GoLast,
        Cmd::SwitchPanel,
        Cmd::ToggleMark,
        Cmd::SelectAll,
        Cmd::InvertSelection,
        Cmd::SelectPattern,
        Cmd::Unmark,
        Cmd::Filter,
        Cmd::Copy,
        Cmd::Move,
        Cmd::MakeDir,
        Cmd::Delete,
        Cmd::DeletePermanently,
        Cmd::EditFile,
        Cmd::NewFile,
        Cmd::TogglePreview,
        Cmd::ToggleTree,
        Cmd::Expand,
        Cmd::Collapse,
        Cmd::ToggleHidden,
        Cmd::Refresh,
        Cmd::SelectTheme,
        Cmd::SelectColumns,
        Cmd::ToolsMenu,
        Cmd::FindFiles,
        Cmd::CancelJobs,
        Cmd::Rename,
        Cmd::YankFiles,
        Cmd::CutFiles,
        Cmd::PasteFiles,
        Cmd::GoTo,
        Cmd::AddBookmark,
        Cmd::OpenTerminal,
        Cmd::CommandPalette,
        Cmd::Help,
        Cmd::Quit,
    ];

    /// Name in menus.
    pub fn label(self) -> &'static str {
        match self {
            Cmd::Open => "Open",
            Cmd::OpenWith => "Open With…",
            Cmd::EditFile => "Edit",
            Cmd::NewFile => "New File…",
            Cmd::MakeDir => "New Folder…",
            Cmd::Rename => "Rename…",
            Cmd::Copy => "Copy to Other Panel",
            Cmd::Move => "Move to Other Panel",
            Cmd::Delete => "Move to Trash",
            Cmd::DeletePermanently => "Delete Permanently",
            Cmd::Quit => "Quit",
            Cmd::CutFiles => "Cut",
            Cmd::YankFiles => "Copy",
            Cmd::PasteFiles => "Paste",
            Cmd::ToggleMark => "Mark / Unmark",
            Cmd::SelectAll => "Select All",
            Cmd::InvertSelection => "Invert Selection",
            Cmd::SelectPattern => "Select by Pattern…",
            Cmd::Unmark => "Clear Filter or Marks",
            Cmd::Filter => "Filter…",
            Cmd::TogglePreview => "Preview",
            Cmd::ToggleTree => "Tree",
            Cmd::Expand => "Expand Folder",
            Cmd::Collapse => "Collapse Folder",
            Cmd::ToggleHidden => "Hidden Files",
            Cmd::SelectColumns => "Columns…",
            Cmd::SelectTheme => "Theme…",
            Cmd::Refresh => "Refresh",
            Cmd::GoUp => "Parent Folder",
            Cmd::GoFirst => "First Entry",
            Cmd::GoLast => "Last Entry",
            Cmd::SwitchPanel => "Other Panel",
            Cmd::GoTo => "Go To… (Bookmarks, Places, Frequent)",
            Cmd::AddBookmark => "Bookmark This Folder",
            Cmd::OpenTerminal => "Terminal Here",
            Cmd::ToolsMenu => "Tools for Selection…",
            Cmd::FindFiles => "Find Files…",
            Cmd::CancelJobs => "Cancel Running Jobs",
            Cmd::CommandPalette => "Command Palette…",
            Cmd::Help => "Keyboard Shortcuts",
        }
    }

    /// Name in the command palette, where there is no menu around it to
    /// give context (and toggles show no check mark).
    pub fn palette_label(self) -> &'static str {
        match self {
            Cmd::Open => "Open / enter directory",
            Cmd::EditFile => "Edit file",
            Cmd::YankFiles => "Copy (yank) entries for pasting",
            Cmd::CutFiles => "Cut entries for pasting",
            Cmd::PasteFiles => "Paste copied / cut entries here",
            Cmd::Rename => "Rename (bulk when several are marked)",
            Cmd::TogglePreview => "Toggle preview",
            Cmd::ToggleTree => "Toggle tree mode",
            Cmd::ToggleHidden => "Toggle hidden files",
            Cmd::Expand => "Expand directory (tree)",
            Cmd::Collapse => "Collapse directory (tree)",
            Cmd::GoUp => "Go to parent directory",
            Cmd::GoFirst => "Go to first entry",
            Cmd::GoLast => "Go to last entry",
            Cmd::SwitchPanel => "Switch panel",
            Cmd::Help => "Help: commands and keys",
            other => other.label(),
        }
    }

    /// For toggles: whether it is on.
    pub fn checked(self, state: &State) -> Option<bool> {
        match self {
            Cmd::TogglePreview => Some(state.preview),
            Cmd::ToggleTree => Some(state.tree),
            Cmd::ToggleHidden => Some(state.hidden),
            _ => None,
        }
    }

    /// Why the command cannot run now, if it cannot.
    pub fn unavailable(self, state: &State) -> Option<&'static str> {
        match self {
            Cmd::OpenWith | Cmd::EditFile if !state.cursor_on_file => Some("Select a file"),
            Cmd::Rename
            | Cmd::Copy
            | Cmd::Move
            | Cmd::Delete
            | Cmd::DeletePermanently
            | Cmd::CutFiles
            | Cmd::YankFiles
                if !state.has_targets =>
            {
                Some("Select or mark entries")
            }
            Cmd::PasteFiles if !state.can_paste => Some("Copy or cut entries first"),
            Cmd::PasteFiles | Cmd::NewFile | Cmd::MakeDir | Cmd::AddBookmark
                if !state.in_directory =>
            {
                Some("Not in a folder")
            }
            Cmd::Expand | Cmd::Collapse if !state.tree => Some("Only in tree mode"),
            Cmd::CancelJobs if !state.jobs_running => Some("No running jobs"),
            _ => None,
        }
    }
}

/// An entry of a menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    Cmd(Cmd),
    Separator,
}

/// The menu bar: menu names and their items.
pub fn menu_bar() -> Vec<(&'static str, Vec<Item>)> {
    use Cmd::*;
    let items = |groups: &[&[Cmd]]| {
        let mut items = Vec::new();
        for (ix, group) in groups.iter().enumerate() {
            if ix > 0 {
                items.push(Item::Separator);
            }
            items.extend(group.iter().map(|&c| Item::Cmd(c)));
        }
        items
    };
    vec![
        (
            "File",
            items(&[
                &[Open, OpenWith, EditFile],
                &[NewFile, MakeDir, Rename],
                &[Copy, Move],
                &[Delete, DeletePermanently],
                &[Quit],
            ]),
        ),
        (
            "Edit",
            items(&[
                &[CutFiles, YankFiles, PasteFiles],
                &[ToggleMark, SelectAll, InvertSelection, SelectPattern],
                &[Unmark, Filter],
            ]),
        ),
        (
            "View",
            items(&[
                &[TogglePreview, ToggleTree, ToggleHidden],
                &[Expand, Collapse],
                &[SelectColumns, SelectTheme],
                &[Refresh],
            ]),
        ),
        (
            "Go",
            items(&[
                &[GoUp, GoFirst, GoLast, SwitchPanel],
                &[GoTo, AddBookmark],
                &[OpenTerminal],
            ]),
        ),
        ("Tools", items(&[&[ToolsMenu, FindFiles], &[CancelJobs]])),
        ("Help", items(&[&[Help, CommandPalette]])),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_is_listed_once() {
        let mut seen = std::collections::HashSet::new();
        for cmd in Cmd::ALL {
            assert!(seen.insert(cmd), "{cmd:?} twice");
        }
        // Every menu item is a known command, and appears once.
        let mut in_menus = std::collections::HashSet::new();
        for (_, items) in menu_bar() {
            for item in items {
                if let Item::Cmd(cmd) = item {
                    assert!(seen.contains(&cmd), "{cmd:?}");
                    assert!(in_menus.insert(cmd), "{cmd:?} in two menus");
                }
            }
        }
    }

    #[test]
    fn menus_have_no_stray_separators() {
        for (name, items) in menu_bar() {
            assert!(!items.is_empty(), "{name}");
            assert_ne!(items.first(), Some(&Item::Separator), "{name}");
            assert_ne!(items.last(), Some(&Item::Separator), "{name}");
            assert!(
                !items
                    .windows(2)
                    .any(|w| w == [Item::Separator, Item::Separator]),
                "{name}"
            );
        }
    }

    #[test]
    fn availability_and_toggles_follow_state() {
        let idle = State {
            in_directory: true,
            ..State::default()
        };
        assert_eq!(Cmd::Copy.unavailable(&idle), Some("Select or mark entries"));
        assert_eq!(Cmd::EditFile.unavailable(&idle), Some("Select a file"));
        assert_eq!(
            Cmd::PasteFiles.unavailable(&idle),
            Some("Copy or cut entries first")
        );
        assert_eq!(Cmd::MakeDir.unavailable(&idle), None);
        assert_eq!(Cmd::Expand.unavailable(&idle), Some("Only in tree mode"));

        let busy = State {
            has_targets: true,
            cursor_on_file: true,
            can_paste: true,
            in_directory: false, // search results
            tree: true,
            preview: true,
            jobs_running: true,
            ..State::default()
        };
        assert_eq!(Cmd::Copy.unavailable(&busy), None);
        assert_eq!(Cmd::EditFile.unavailable(&busy), None);
        assert_eq!(Cmd::PasteFiles.unavailable(&busy), Some("Not in a folder"));
        assert_eq!(Cmd::NewFile.unavailable(&busy), Some("Not in a folder"));
        assert_eq!(Cmd::CancelJobs.unavailable(&busy), None);
        assert_eq!(Cmd::TogglePreview.checked(&busy), Some(true));
        assert_eq!(Cmd::ToggleHidden.checked(&busy), Some(false));
        assert_eq!(Cmd::Copy.checked(&busy), None);
    }
}
