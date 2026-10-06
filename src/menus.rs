//! Commands as the user sees them: names, where they appear in the menu
//! bar, toggle states and availability. UI-independent; the menu bar and
//! the command palette and the context menu are built from this,
//! so names and availability agree everywhere. `commander.rs` maps each
//! [`Cmd`] to its gpui action.

use crate::fs_ops::SortKey;

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
    ExtractArchive,
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
    SortByName,
    SortByExt,
    SortBySize,
    SortByModified,
    SortDescending,
    SelectColumns,
    SelectTheme,
    Refresh,
    GoBack,
    GoForward,
    GoUp,
    GoFirst,
    GoLast,
    SwitchPanel,
    EditLocation,
    CopyLocation,
    GoTo,
    AddBookmark,
    OpenTerminal,
    ToolsMenu,
    FindFiles,
    CancelJobs,
    CommandPalette,
    Help,
    OpenConfig,
    About,
    ZoomIn,
    ZoomOut,
    ZoomReset,
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
    /// The panel browses inside an archive, which is read-only.
    pub in_archive: bool,
    /// The cursor is on an archive file that can be extracted.
    pub cursor_on_archive: bool,
    pub tree: bool,
    pub hidden: bool,
    pub preview: bool,
    pub jobs_running: bool,
    pub sort: crate::model::Sort,
    pub can_go_back: bool,
    pub can_go_forward: bool,
}

impl Cmd {
    /// Every command, in palette order.
    pub const ALL: [Cmd; 55] = [
        Cmd::Open,
        Cmd::OpenWith,
        Cmd::GoBack,
        Cmd::GoForward,
        Cmd::GoUp,
        Cmd::GoFirst,
        Cmd::GoLast,
        Cmd::SwitchPanel,
        Cmd::EditLocation,
        Cmd::CopyLocation,
        Cmd::ToggleMark,
        Cmd::SelectAll,
        Cmd::InvertSelection,
        Cmd::SelectPattern,
        Cmd::Unmark,
        Cmd::Filter,
        Cmd::Copy,
        Cmd::Move,
        Cmd::ExtractArchive,
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
        Cmd::SortByName,
        Cmd::SortByExt,
        Cmd::SortBySize,
        Cmd::SortByModified,
        Cmd::SortDescending,
        Cmd::Refresh,
        Cmd::ZoomIn,
        Cmd::ZoomOut,
        Cmd::ZoomReset,
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
        Cmd::OpenConfig,
        Cmd::About,
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
            Cmd::ExtractArchive => "Extract Archive to Other Panel",
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
            Cmd::SortByName => "By Name",
            Cmd::SortByExt => "By Extension",
            Cmd::SortBySize => "By Size",
            Cmd::SortByModified => "By Modified",
            Cmd::SortDescending => "Descending",
            Cmd::SelectColumns => "Columns…",
            Cmd::SelectTheme => "Theme…",
            Cmd::Refresh => "Refresh",
            Cmd::GoBack => "Back",
            Cmd::GoForward => "Forward",
            Cmd::GoUp => "Parent Folder",
            Cmd::GoFirst => "First Entry",
            Cmd::GoLast => "Last Entry",
            Cmd::SwitchPanel => "Other Panel",
            Cmd::EditLocation => "Edit Location…",
            Cmd::CopyLocation => "Copy Full Path",
            Cmd::GoTo => "Go To… (Bookmarks, Places, Frequent)",
            Cmd::AddBookmark => "Bookmark This Folder",
            Cmd::OpenTerminal => "Terminal Here",
            Cmd::ToolsMenu => "Tools for Selection…",
            Cmd::FindFiles => "Find Files…",
            Cmd::CancelJobs => "Cancel Running Jobs",
            Cmd::CommandPalette => "Command Palette…",
            Cmd::Help => "Keyboard Shortcuts",
            Cmd::OpenConfig => "Configuration",
            Cmd::About => "About Fyle",
            Cmd::ZoomIn => "Zoom In",
            Cmd::ZoomOut => "Zoom Out",
            Cmd::ZoomReset => "Reset Zoom",
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
            Cmd::ExtractArchive => "Extract archive into a folder in the other panel",
            Cmd::TogglePreview => "Toggle preview",
            Cmd::ToggleTree => "Toggle tree mode",
            Cmd::ToggleHidden => "Toggle hidden files",
            Cmd::SortByName => "Sort by name",
            Cmd::SortByExt => "Sort by extension",
            Cmd::SortBySize => "Sort by size",
            Cmd::SortByModified => "Sort by modified time",
            Cmd::SortDescending => "Toggle descending sort",
            Cmd::Expand => "Expand directory (tree)",
            Cmd::Collapse => "Collapse directory (tree)",
            Cmd::GoBack => "Go back (previous directory)",
            Cmd::GoForward => "Go forward",
            Cmd::GoUp => "Go to parent directory",
            Cmd::GoFirst => "Go to first entry",
            Cmd::GoLast => "Go to last entry",
            Cmd::SwitchPanel => "Switch panel",
            Cmd::EditLocation => "Edit or paste the active panel location",
            Cmd::CopyLocation => "Copy the active panel's full path",
            Cmd::Help => "Help: commands and keys",
            Cmd::OpenConfig => "Edit configuration (config.toml)",
            Cmd::ZoomIn => "Zoom in UI",
            Cmd::ZoomOut => "Zoom out UI",
            Cmd::ZoomReset => "Reset UI zoom",
            other => other.label(),
        }
    }

    /// For toggles: whether it is on.
    pub fn checked(self, state: &State) -> Option<bool> {
        match self {
            Cmd::TogglePreview => Some(state.preview),
            Cmd::ToggleTree => Some(state.tree),
            Cmd::ToggleHidden => Some(state.hidden),
            Cmd::SortDescending => Some(state.sort.descending),
            cmd => cmd.sort_key().map(|key| state.sort.key == key),
        }
    }

    /// For the sort commands: the order they select.
    pub fn sort_key(self) -> Option<SortKey> {
        match self {
            Cmd::SortByName => Some(SortKey::Name),
            Cmd::SortByExt => Some(SortKey::Ext),
            Cmd::SortBySize => Some(SortKey::Size),
            Cmd::SortByModified => Some(SortKey::Modified),
            _ => None,
        }
    }

    /// Why the command cannot run now, if it cannot.
    pub fn unavailable(self, state: &State) -> Option<&'static str> {
        const READ_ONLY: &str = "Archives are read-only (F5 extracts)";
        match self {
            Cmd::Rename
            | Cmd::Move
            | Cmd::Delete
            | Cmd::DeletePermanently
            | Cmd::CutFiles
            | Cmd::YankFiles
            | Cmd::PasteFiles
            | Cmd::NewFile
            | Cmd::MakeDir
            | Cmd::EditFile
                if state.in_archive =>
            {
                Some(READ_ONLY)
            }
            Cmd::OpenWith
            | Cmd::AddBookmark
            | Cmd::OpenTerminal
            | Cmd::ToolsMenu
            | Cmd::FindFiles
                if state.in_archive =>
            {
                Some("Not available inside an archive")
            }
            Cmd::ExtractArchive if !state.cursor_on_archive => Some("Select an archive file"),
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
            Cmd::GoBack if !state.can_go_back => Some("No previous folder"),
            Cmd::GoForward if !state.can_go_forward => Some("No next folder"),
            _ => None,
        }
    }
}

/// An entry of a menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    Cmd(Cmd),
    Separator,
    Submenu(&'static str, &'static [Cmd]),
}

/// File actions shared with the menu bar and palette, grouped for right-click.
pub fn context_menu() -> Vec<Item> {
    use Cmd::*;
    use Item::{Cmd as Command, Separator};
    vec![
        Command(Open),
        Command(OpenWith),
        Command(EditFile),
        Separator,
        Command(CutFiles),
        Command(YankFiles),
        Command(PasteFiles),
        Separator,
        Command(Copy),
        Command(Move),
        Command(ExtractArchive),
        Command(Rename),
        Separator,
        Command(Delete),
        Command(DeletePermanently),
        Separator,
        Command(NewFile),
        Command(MakeDir),
        Separator,
        Command(ToolsMenu),
    ]
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
                &[Copy, Move, ExtractArchive],
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
        ("View", {
            let mut view = items(&[
                &[TogglePreview, ToggleTree, ToggleHidden],
                &[Expand, Collapse],
            ]);
            view.push(Item::Separator);
            view.push(Item::Submenu(
                "Sort",
                &[
                    SortByName,
                    SortByExt,
                    SortBySize,
                    SortByModified,
                    SortDescending,
                ],
            ));
            view.extend(items(&[
                &[],
                &[ZoomIn, ZoomOut, ZoomReset],
                &[SelectColumns, SelectTheme],
                &[Refresh],
            ]));
            view
        }),
        (
            "Go",
            items(&[
                &[GoBack, GoForward, GoUp],
                &[GoFirst, GoLast, SwitchPanel],
                &[EditLocation, CopyLocation, GoTo, AddBookmark],
                &[OpenTerminal],
            ]),
        ),
        ("Tools", items(&[&[ToolsMenu, FindFiles], &[CancelJobs]])),
        (
            "Help",
            items(&[&[Help, CommandPalette], &[OpenConfig], &[About]]),
        ),
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
                let cmds: &[Cmd] = match &item {
                    Item::Cmd(cmd) => std::slice::from_ref(cmd),
                    Item::Submenu(_, cmds) => cmds,
                    Item::Separator => &[],
                };
                for &cmd in cmds {
                    assert!(seen.contains(&cmd), "{cmd:?}");
                    assert!(in_menus.insert(cmd), "{cmd:?} in two menus");
                }
            }
        }
    }

    #[test]
    fn menus_have_no_stray_separators() {
        for (name, items) in menu_bar().into_iter().chain([("Context", context_menu())]) {
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
    fn context_actions_share_definitions_and_availability() {
        let commands: Vec<_> = context_menu()
            .into_iter()
            .filter_map(|item| match item {
                Item::Cmd(cmd) => Some(cmd),
                _ => None,
            })
            .collect();
        let mut seen = std::collections::HashSet::new();
        for &cmd in &commands {
            assert!(Cmd::ALL.contains(&cmd));
            assert!(seen.insert(cmd));
        }
        let parent = State {
            in_directory: true,
            ..State::default()
        };
        for cmd in [
            Cmd::Rename,
            Cmd::Copy,
            Cmd::Move,
            Cmd::Delete,
            Cmd::DeletePermanently,
            Cmd::CutFiles,
            Cmd::YankFiles,
            Cmd::OpenWith,
            Cmd::EditFile,
            Cmd::PasteFiles,
        ] {
            assert!(commands.contains(&cmd));
            assert!(cmd.unavailable(&parent).is_some(), "{cmd:?}");
        }
        for cmd in [Cmd::Open, Cmd::NewFile, Cmd::MakeDir] {
            assert!(commands.contains(&cmd));
            assert!(cmd.unavailable(&parent).is_none());
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

        // Inside an archive only reading commands remain.
        let archive = State {
            has_targets: true,
            cursor_on_file: true,
            in_directory: true,
            in_archive: true,
            ..State::default()
        };
        for cmd in [
            Cmd::Rename,
            Cmd::Move,
            Cmd::Delete,
            Cmd::DeletePermanently,
            Cmd::CutFiles,
            Cmd::PasteFiles,
            Cmd::NewFile,
            Cmd::MakeDir,
            Cmd::EditFile,
            Cmd::OpenWith,
        ] {
            assert!(cmd.unavailable(&archive).is_some(), "{cmd:?}");
        }
        assert_eq!(Cmd::Copy.unavailable(&archive), None);
        assert_eq!(Cmd::TogglePreview.unavailable(&archive), None);
        assert_eq!(
            Cmd::ExtractArchive.unavailable(&idle),
            Some("Select an archive file")
        );
        let on_archive = State {
            cursor_on_archive: true,
            ..idle.clone()
        };
        assert_eq!(Cmd::ExtractArchive.unavailable(&on_archive), None);

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
        // Sort: the current key is checked.
        assert_eq!(Cmd::SortByName.checked(&busy), Some(true));
        assert_eq!(Cmd::SortBySize.checked(&busy), Some(false));
        assert_eq!(Cmd::SortDescending.checked(&busy), Some(false));
    }
}
