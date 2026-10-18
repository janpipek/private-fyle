//! Commands as the user sees them: names, where they appear in the menu
//! bar, toggle states and availability. UI-independent; the menu bar and
//! the command palette and the context menu are built from this,
//! so names and availability agree everywhere. `commander.rs` maps each
//! [`Cmd`] to its gpui action.

use crate::entry::SortKey;
use crate::grid::{self, ViewMode};
use crate::remote::{Capabilities, Removal};
use crate::workspace::LayoutMode;

/// A user-visible command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cmd {
    Open,
    OpenWith,
    OpenInBrowser,
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
    ClearSelection,
    Filter,
    TogglePreview,
    ShowProperties,
    LayoutTwoPanels,
    LayoutTwoPanelsInspector,
    LayoutOnePanelInspector,
    ToggleTree,
    ListView,
    IconView,
    LargerIcons,
    SmallerIcons,
    Expand,
    Collapse,
    ToggleHidden,
    SortByName,
    SortByExt,
    SortBySize,
    SortByModified,
    SortDescending,
    SelectColumns,
    RemoveColumn,
    FilterColumn,
    ClearColumnFilter,
    ClearColumnFilters,
    SelectTheme,
    ToggleCustomTitlebar,
    Refresh,
    CalculateSizes,
    GoBack,
    GoForward,
    GoUp,
    GoFirst,
    GoLast,
    SwitchPanel,
    EditLocation,
    CopyLocation,
    CopyFullPath,
    CopyName,
    GoTo,
    GoToOtherFolder,
    SwapPanels,
    ToggleBookmark,
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
    EditShortcuts,
    ManageRemotes,
}

/// What availability and toggle states depend on, taken from the window.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    /// Something to act on: marked entries, or a cursor entry other than "..".
    pub has_targets: bool,
    /// Cursor points to a file or directory rather than the parent row.
    pub cursor_on_entry: bool,
    /// The selected entry has a native path or a standard remote URL.
    pub selected_path_available: bool,
    /// At least one entry of the active panel is marked.
    pub has_marks: bool,
    /// Why the active panel cannot follow the other panel's folder.
    pub other_folder_blocked: Option<&'static str>,
    /// Why the two panels cannot be swapped now.
    pub swap_blocked: Option<&'static str>,
    /// The cursor is on a regular file (not a directory).
    pub cursor_on_file: bool,
    /// Local file or directory that the Open With submenu will act on.
    pub open_with_path: Option<std::path::PathBuf>,
    /// Entries were yanked or cut and can be pasted.
    pub can_paste: bool,
    /// The panel lists a directory (not search results or the drive list).
    pub in_directory: bool,
    /// The active panel's folder is in the bookmarks.
    pub bookmarked: bool,
    /// The panel browses inside an archive, which is read-only.
    pub in_archive: bool,
    /// There are folders to measure: marked ones, the cursor's, or listed.
    pub has_folders: bool,
    /// The cursor is on an archive file that can be extracted.
    pub cursor_on_archive: bool,
    pub tree: bool,
    /// How the active panel shows its folder, and its icon size.
    pub view: ViewMode,
    pub icon_size: u32,
    pub hidden: bool,
    pub preview: bool,
    /// Whether the Properties perspective is selected in the inspector.
    pub properties: bool,
    /// The selected folder/inspector arrangement.
    pub workspace_layout: LayoutMode,
    pub jobs_running: bool,
    pub sort: crate::entry::Sort,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    /// What the remote shown by the active panel supports; `None` for
    /// local folders and archives.
    pub remote: Option<Capabilities>,
    /// Why Copy to the other panel cannot run because of a remote on
    /// either side (see [`crate::remote::jobs::plan_transfer`]).
    pub copy_blocked: Option<&'static str>,
    /// Likewise for Move.
    pub move_blocked: Option<&'static str>,
    /// Custom title bar preference for the next application start.
    pub custom_titlebar: bool,
    /// Whether a custom title bar preference save is underway.
    pub custom_titlebar_saving: bool,
    /// Header targeted by a column context menu; absent for global commands.
    pub header_column: Option<crate::columns::ColumnId>,
    /// The captured header column already has an active filter.
    pub header_filter_active: bool,
    /// The active panel has one or more column filters.
    pub has_column_filters: bool,
    /// Why the captured column cannot currently be filtered.
    pub filter_column_unavailable: Option<&'static str>,
}

impl Cmd {
    /// Every command, in palette order.
    pub const ALL: [Cmd; 75] = [
        Cmd::Open,
        Cmd::OpenWith,
        Cmd::OpenInBrowser,
        Cmd::GoBack,
        Cmd::GoForward,
        Cmd::GoUp,
        Cmd::GoFirst,
        Cmd::GoLast,
        Cmd::SwitchPanel,
        Cmd::EditLocation,
        Cmd::CopyLocation,
        Cmd::CopyFullPath,
        Cmd::CopyName,
        Cmd::ToggleMark,
        Cmd::SelectAll,
        Cmd::InvertSelection,
        Cmd::SelectPattern,
        Cmd::Unmark,
        Cmd::ClearSelection,
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
        Cmd::ShowProperties,
        Cmd::LayoutTwoPanels,
        Cmd::LayoutTwoPanelsInspector,
        Cmd::LayoutOnePanelInspector,
        Cmd::ToggleTree,
        Cmd::ListView,
        Cmd::IconView,
        Cmd::LargerIcons,
        Cmd::SmallerIcons,
        Cmd::Expand,
        Cmd::Collapse,
        Cmd::ToggleHidden,
        Cmd::SortByName,
        Cmd::SortByExt,
        Cmd::SortBySize,
        Cmd::SortByModified,
        Cmd::SortDescending,
        Cmd::Refresh,
        Cmd::CalculateSizes,
        Cmd::ZoomIn,
        Cmd::ZoomOut,
        Cmd::ZoomReset,
        Cmd::SelectTheme,
        Cmd::ToggleCustomTitlebar,
        Cmd::SelectColumns,
        Cmd::RemoveColumn,
        Cmd::ClearColumnFilters,
        Cmd::ToolsMenu,
        Cmd::FindFiles,
        Cmd::CancelJobs,
        Cmd::Rename,
        Cmd::YankFiles,
        Cmd::CutFiles,
        Cmd::PasteFiles,
        Cmd::GoTo,
        Cmd::GoToOtherFolder,
        Cmd::SwapPanels,
        Cmd::ToggleBookmark,
        Cmd::OpenTerminal,
        Cmd::CommandPalette,
        Cmd::Help,
        Cmd::OpenConfig,
        Cmd::About,
        Cmd::Quit,
        Cmd::EditShortcuts,
        Cmd::ManageRemotes,
    ];

    /// Name in menus.
    pub fn label(self) -> &'static str {
        match self {
            Cmd::Open => "Open",
            Cmd::OpenWith => "Open With…",
            Cmd::OpenInBrowser => "Open in Browser",
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
            Cmd::ClearSelection => "Clear Selection",
            Cmd::Filter => "Filter…",
            Cmd::TogglePreview => "Preview",
            Cmd::ShowProperties => "Properties",
            Cmd::LayoutTwoPanels => "Two Panels",
            Cmd::LayoutTwoPanelsInspector => "Two Panels + Preview",
            Cmd::LayoutOnePanelInspector => "One Panel + Preview",
            Cmd::ToggleTree => "Tree",
            Cmd::ListView => "List View",
            Cmd::IconView => "Icon View",
            Cmd::LargerIcons => "Larger Icons",
            Cmd::SmallerIcons => "Smaller Icons",
            Cmd::Expand => "Expand Folder",
            Cmd::Collapse => "Collapse Folder",
            Cmd::ToggleHidden => "Hidden Files",
            Cmd::SortByName => "By Name",
            Cmd::SortByExt => "By Extension",
            Cmd::SortBySize => "By Size",
            Cmd::SortByModified => "By Modified",
            Cmd::SortDescending => "Descending",
            Cmd::SelectColumns => "Edit columns…",
            Cmd::RemoveColumn => "Remove column",
            Cmd::FilterColumn => "Filter column…",
            Cmd::ClearColumnFilter => "Clear column filter",
            Cmd::ClearColumnFilters => "Clear column filters",
            Cmd::SelectTheme => "Theme…",
            Cmd::ToggleCustomTitlebar => "Custom Title Bar",
            Cmd::Refresh => "Refresh",
            Cmd::CalculateSizes => "Calculate Sizes",
            Cmd::GoBack => "Back",
            Cmd::GoForward => "Forward",
            Cmd::GoUp => "Parent Folder",
            Cmd::GoFirst => "First Entry",
            Cmd::GoLast => "Last Entry",
            Cmd::SwitchPanel => "Other Panel",
            Cmd::EditLocation => "Edit Location…",
            Cmd::CopyLocation => "Copy Panel Location",
            Cmd::CopyFullPath => "Copy Full Path",
            Cmd::CopyName => "Copy Name",
            Cmd::GoTo => "Go To… (Bookmarks, Places, Frequent)",
            Cmd::GoToOtherFolder => "Go to Other Panel's Folder",
            Cmd::SwapPanels => "Swap Panels",
            Cmd::ToggleBookmark => "Bookmark This Folder",
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
            Cmd::EditShortcuts => "Edit Keyboard Shortcuts…",
            Cmd::ManageRemotes => "Manage Remotes…",
        }
    }

    /// Name in the command palette, where there is no menu around it to
    /// give context (and toggles show no check mark).
    pub fn palette_label(self) -> &'static str {
        match self {
            Cmd::Open => "Open / enter directory",
            Cmd::OpenInBrowser => "Open the remote item (or folder) in the browser",
            Cmd::EditFile => "Edit file",
            Cmd::YankFiles => "Copy (yank) entries for pasting",
            Cmd::CutFiles => "Cut entries for pasting",
            Cmd::PasteFiles => "Paste copied / cut entries here",
            Cmd::Rename => "Rename (bulk when several are marked)",
            Cmd::ExtractArchive => "Extract archive into a folder in the other panel",
            Cmd::TogglePreview => "Toggle preview",
            Cmd::ShowProperties => "Show Properties",
            Cmd::ToggleCustomTitlebar => "Custom Title Bar (restart required)",
            Cmd::LayoutTwoPanels => "Show both folder panels",
            Cmd::LayoutTwoPanelsInspector => "Show both panels and preview",
            Cmd::LayoutOnePanelInspector => "Show the active panel and preview",
            Cmd::ToggleTree => "Toggle tree mode",
            Cmd::ListView => "Show the active panel as a list",
            Cmd::IconView => "Show the active panel as icons",
            Cmd::LargerIcons => "Larger icons in icon view",
            Cmd::SmallerIcons => "Smaller icons in icon view",
            Cmd::ToggleHidden => "Toggle hidden files",
            Cmd::SortByName => "Sort by name",
            Cmd::SortByExt => "Sort by extension",
            Cmd::SortBySize => "Sort by size",
            Cmd::SortByModified => "Sort by modified time",
            Cmd::SortDescending => "Toggle descending sort",
            Cmd::Expand => "Expand directory (tree)",
            Cmd::Collapse => "Collapse directory (tree)",
            Cmd::CalculateSizes => "Calculate folder sizes (marked, cursor or all folders)",
            Cmd::GoBack => "Go back (previous directory)",
            Cmd::GoForward => "Go forward",
            Cmd::GoUp => "Go to parent directory",
            Cmd::GoFirst => "Go to first entry",
            Cmd::GoLast => "Go to last entry",
            Cmd::ClearSelection => "Clear the marks of the active panel",
            Cmd::SwitchPanel => "Switch panel",
            Cmd::ToggleBookmark => "Add or remove the bookmark for this folder",
            Cmd::EditLocation => "Edit or paste the active panel location",
            Cmd::CopyLocation => "Copy the active panel's full path",
            Cmd::CopyFullPath => "Copy the selected entry's full path",
            Cmd::CopyName => "Copy the name of the entry under the cursor",
            Cmd::Help => "Help: commands and keys",
            Cmd::OpenConfig => "Edit configuration (config.toml)",
            Cmd::EditShortcuts => "Edit keyboard shortcuts",
            Cmd::ManageRemotes => "Manage remotes in remotes.toml",
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
            Cmd::ShowProperties => Some(state.properties),
            Cmd::LayoutTwoPanels => Some(state.workspace_layout == LayoutMode::TwoPanels),
            Cmd::LayoutTwoPanelsInspector => {
                Some(state.workspace_layout == LayoutMode::TwoPanelsInspector)
            }
            Cmd::LayoutOnePanelInspector => {
                Some(state.workspace_layout == LayoutMode::OnePanelInspector)
            }
            Cmd::ToggleTree => Some(state.tree),
            Cmd::ListView => Some(state.view == ViewMode::List),
            Cmd::IconView => Some(state.view == ViewMode::Icons),
            Cmd::ToggleHidden => Some(state.hidden),
            Cmd::ToggleBookmark => Some(state.bookmarked),
            Cmd::ToggleCustomTitlebar => Some(state.custom_titlebar),
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

    /// Why the command cannot run on the remote folder the panel shows.
    fn remote_unavailable(self, caps: &Capabilities) -> Option<&'static str> {
        match self {
            Cmd::OpenWith
            | Cmd::EditFile
            | Cmd::NewFile
            | Cmd::OpenTerminal
            | Cmd::ToolsMenu
            | Cmd::FindFiles
            | Cmd::CalculateSizes
            | Cmd::ExtractArchive
            | Cmd::CutFiles
            | Cmd::YankFiles
            | Cmd::PasteFiles
            | Cmd::ToggleTree
            | Cmd::Expand
            | Cmd::Collapse => Some("Not available on remote folders"),
            Cmd::OpenInBrowser if !caps.web => Some("This remote has no web view"),
            Cmd::Rename if !caps.rename => Some("This remote does not support renaming"),
            Cmd::MakeDir if !caps.create_folder => {
                Some("This remote does not support creating folders")
            }
            Cmd::Delete | Cmd::DeletePermanently if caps.removal.is_none() => {
                Some("This remote does not support removing items")
            }
            Cmd::DeletePermanently if caps.removal == Some(Removal::Trash) => {
                Some("This remote moves deleted items to its trash")
            }
            Cmd::DeletePermanently => Some("Delete already removes items for good on this remote"),
            _ => None,
        }
    }

    /// Why the command cannot run now, if it cannot.
    pub fn unavailable(self, state: &State) -> Option<&'static str> {
        if self == Cmd::RemoveColumn {
            return match state
                .header_column
                .as_ref()
                .map(crate::columns::ColumnId::builtin)
            {
                None => Some("Right-click a column header"),
                Some(Some(crate::columns::ColumnKind::Name)) => Some("Name is required"),
                Some(_) => None,
            };
        }
        if self == Cmd::FilterColumn {
            return if state.header_column.is_none() {
                Some("Right-click a column header")
            } else {
                state.filter_column_unavailable
            };
        }
        if self == Cmd::ClearColumnFilter {
            if state.header_column.is_none() {
                return Some("Right-click a column header");
            }
            if !state.header_filter_active {
                return Some("This column has no filter");
            }
        }
        if self == Cmd::ClearColumnFilters && !state.has_column_filters {
            return Some("There are no column filters");
        }
        if self == Cmd::ToggleCustomTitlebar {
            if cfg!(target_os = "macos") {
                return Some("Custom title bars are not available on macOS");
            }
            if state.custom_titlebar_saving {
                return Some("Saving custom title bar preference");
            }
        }
        if let Some(caps) = &state.remote
            && let Some(reason) = self.remote_unavailable(caps)
        {
            return Some(reason);
        }
        if self == Cmd::OpenInBrowser && state.remote.is_none() {
            return Some("Not a remote folder");
        }
        match self {
            Cmd::Copy if state.copy_blocked.is_some() => return state.copy_blocked,
            Cmd::Move if state.move_blocked.is_some() => return state.move_blocked,
            _ => {}
        }
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
            | Cmd::ToggleBookmark
            | Cmd::OpenTerminal
            | Cmd::ToolsMenu
            | Cmd::FindFiles
            | Cmd::CalculateSizes
                if state.in_archive =>
            {
                Some("Not available inside an archive")
            }
            Cmd::CalculateSizes if !state.has_folders => Some("No folders to measure"),
            Cmd::ExtractArchive if !state.cursor_on_archive => Some("Select an archive file"),
            Cmd::OpenWith if state.open_with_path.is_none() => Some("Select a file or directory"),
            Cmd::EditFile if !state.cursor_on_file => Some("Select a file"),
            Cmd::CopyName if !state.cursor_on_entry => Some("Select a file or directory"),
            Cmd::ShowProperties if !state.cursor_on_entry => Some("Select a file or directory"),
            Cmd::CopyFullPath if state.selected_path_available => None,
            Cmd::CopyFullPath if !state.cursor_on_entry => Some("Select a file or directory"),
            Cmd::CopyFullPath => Some("No standard address is available for this entry"),
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
            Cmd::GoToOtherFolder if state.other_folder_blocked.is_some() => {
                state.other_folder_blocked
            }
            Cmd::SwapPanels if state.swap_blocked.is_some() => state.swap_blocked,
            Cmd::ClearSelection if !state.has_marks => Some("Nothing is marked"),
            Cmd::PasteFiles if !state.can_paste => Some("Copy or cut entries first"),
            Cmd::PasteFiles | Cmd::NewFile | Cmd::MakeDir | Cmd::ToggleBookmark
                if !state.in_directory =>
            {
                Some("Not in a folder")
            }
            Cmd::ToggleTree | Cmd::Expand | Cmd::Collapse if state.view == ViewMode::Icons => {
                Some("Tree mode only applies to the list view")
            }
            Cmd::LargerIcons | Cmd::SmallerIcons if state.view != ViewMode::Icons => {
                Some("Only in icon view")
            }
            Cmd::LargerIcons if grid::step_size(state.icon_size, 1) == state.icon_size => {
                Some("Already the largest size")
            }
            Cmd::SmallerIcons if grid::step_size(state.icon_size, -1) == state.icon_size => {
                Some("Already the smallest size")
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
    OpenWith,
    Separator,
    Submenu(&'static str, &'static [Cmd]),
}

/// File actions shared with the menu bar and palette, grouped for right-click.
pub fn context_menu() -> Vec<Item> {
    use Cmd::*;
    use Item::{Cmd as Command, OpenWith, Separator};
    vec![
        Command(Open),
        OpenWith,
        Command(EditFile),
        Command(ShowProperties),
        Separator,
        Command(CutFiles),
        Command(YankFiles),
        Command(CopyName),
        Command(CopyFullPath),
        Command(PasteFiles),
        Item::Submenu(
            "Selection",
            &[
                ToggleMark,
                SelectAll,
                InvertSelection,
                SelectPattern,
                ClearSelection,
                Unmark,
                Filter,
            ],
        ),
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
        Command(CalculateSizes),
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
    let mut file = items(&[&[Open, ShowProperties]]);
    file.push(Item::OpenWith);
    file.extend(items(&[
        &[OpenInBrowser, EditFile],
        &[NewFile, MakeDir, Rename],
        &[Copy, Move, ExtractArchive],
        &[Delete, DeletePermanently],
        &[Quit],
    ]));
    vec![
        ("File", file),
        (
            "Edit",
            items(&[
                &[CutFiles, YankFiles, CopyName, PasteFiles],
                &[ToggleMark, SelectAll, InvertSelection, SelectPattern],
                &[ClearSelection, Unmark, Filter],
            ]),
        ),
        ("View", {
            let mut view = items(&[
                &[
                    TogglePreview,
                    LayoutTwoPanels,
                    LayoutTwoPanelsInspector,
                    LayoutOnePanelInspector,
                ],
                &[ToggleTree, ToggleHidden],
                &[ListView, IconView, LargerIcons, SmallerIcons],
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
                &[SelectColumns, SelectTheme, ToggleCustomTitlebar],
                &[CalculateSizes, Refresh],
            ]));
            view
        }),
        (
            "Go",
            items(&[
                &[GoBack, GoForward, GoUp],
                &[GoFirst, GoLast, SwitchPanel],
                &[
                    EditLocation,
                    CopyLocation,
                    GoTo,
                    GoToOtherFolder,
                    SwapPanels,
                    ToggleBookmark,
                    ManageRemotes,
                ],
                &[OpenTerminal],
            ]),
        ),
        (
            "Tools",
            items(&[&[ToolsMenu, FindFiles], &[ManageRemotes], &[CancelJobs]]),
        ),
        (
            "Help",
            items(&[
                &[Help, EditShortcuts, CommandPalette],
                &[OpenConfig],
                &[About],
            ]),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removal_requires_a_header_and_keeps_name_mandatory() {
        use crate::columns::{ColumnId, ColumnKind};
        let mut state = State::default();
        assert_eq!(
            Cmd::RemoveColumn.unavailable(&state),
            Some("Right-click a column header")
        );
        state.header_column = Some(ColumnKind::Name.into());
        assert_eq!(
            Cmd::RemoveColumn.unavailable(&state),
            Some("Name is required")
        );
        for id in [
            ColumnKind::Size.into(),
            ColumnId::new("extension.gone.value").unwrap(),
        ] {
            state.header_column = Some(id);
            assert_eq!(Cmd::RemoveColumn.unavailable(&state), None);
            assert_eq!(Cmd::RemoveColumn.checked(&state), None);
        }
    }

    #[test]
    fn column_filter_commands_follow_header_and_rule_availability() {
        use crate::columns::ColumnKind;
        let mut state = State::default();
        assert_eq!(
            Cmd::FilterColumn.unavailable(&state),
            Some("Right-click a column header")
        );
        assert_eq!(
            Cmd::ClearColumnFilter.unavailable(&state),
            Some("Right-click a column header")
        );
        assert_eq!(
            Cmd::ClearColumnFilters.unavailable(&state),
            Some("There are no column filters")
        );
        state.header_column = Some(ColumnKind::Owner.into());
        state.filter_column_unavailable = Some("This column requires local files");
        assert_eq!(
            Cmd::FilterColumn.unavailable(&state),
            state.filter_column_unavailable
        );
        state.filter_column_unavailable = None;
        assert_eq!(Cmd::FilterColumn.unavailable(&state), None);
        assert_eq!(
            Cmd::ClearColumnFilter.unavailable(&state),
            Some("This column has no filter")
        );
        state.header_filter_active = true;
        state.has_column_filters = true;
        assert_eq!(Cmd::ClearColumnFilter.unavailable(&state), None);
        assert_eq!(Cmd::ClearColumnFilters.unavailable(&state), None);
    }

    #[test]
    fn every_command_is_listed_once() {
        let mut seen = std::collections::HashSet::new();
        for cmd in Cmd::ALL {
            assert!(seen.insert(cmd), "{cmd:?} twice");
        }
        // Every menu item is known; Manage Remotes is intentionally offered
        // from both Go and Tools.
        let mut in_menus = std::collections::HashSet::new();
        for (_, items) in menu_bar() {
            for item in items {
                let cmds: &[Cmd] = match &item {
                    Item::Cmd(cmd) => std::slice::from_ref(cmd),
                    Item::OpenWith => &[Cmd::OpenWith],
                    Item::Submenu(_, cmds) => cmds,
                    Item::Separator => &[],
                };
                for &cmd in cmds {
                    assert!(seen.contains(&cmd), "{cmd:?}");
                    if cmd == Cmd::ManageRemotes {
                        continue;
                    }
                    assert!(in_menus.insert(cmd), "{cmd:?} in two menus");
                }
            }
        }
    }

    #[test]
    fn custom_titlebar_command_shows_saved_state_and_busy_availability() {
        let view = menu_bar()
            .into_iter()
            .find_map(|(name, items)| (name == "View").then_some(items))
            .expect("View menu exists");
        assert!(view.contains(&Item::Cmd(Cmd::ToggleCustomTitlebar)));
        assert_eq!(Cmd::ToggleCustomTitlebar.label(), "Custom Title Bar");
        assert!(
            Cmd::ToggleCustomTitlebar
                .palette_label()
                .contains("restart")
        );
        assert_eq!(
            Cmd::ToggleCustomTitlebar.checked(&State {
                custom_titlebar: false,
                ..State::default()
            }),
            Some(false)
        );
        if cfg!(target_os = "macos") {
            assert_eq!(
                Cmd::ToggleCustomTitlebar.unavailable(&State::default()),
                Some("Custom title bars are not available on macOS")
            );
        } else {
            assert_eq!(
                Cmd::ToggleCustomTitlebar.unavailable(&State {
                    custom_titlebar_saving: true,
                    ..State::default()
                }),
                Some("Saving custom title bar preference")
            );
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
    fn workspace_layout_commands_are_view_toggles() {
        let layouts = [
            (Cmd::LayoutTwoPanels, LayoutMode::TwoPanels),
            (
                Cmd::LayoutTwoPanelsInspector,
                LayoutMode::TwoPanelsInspector,
            ),
            (Cmd::LayoutOnePanelInspector, LayoutMode::OnePanelInspector),
        ];
        let view = menu_bar()
            .into_iter()
            .find_map(|(name, items)| (name == "View").then_some(items))
            .expect("View menu exists");

        for (cmd, mode) in layouts {
            assert!(view.contains(&Item::Cmd(cmd)));
            assert!(Cmd::ALL.contains(&cmd));
            assert!(cmd.label().contains("Panel"));
            assert!(!cmd.palette_label().is_empty());
            assert_eq!(
                cmd.checked(&State {
                    workspace_layout: mode,
                    ..State::default()
                }),
                Some(true)
            );
            for other in [
                LayoutMode::TwoPanels,
                LayoutMode::TwoPanelsInspector,
                LayoutMode::OnePanelInspector,
            ] {
                assert_eq!(
                    cmd.checked(&State {
                        workspace_layout: other,
                        ..State::default()
                    }),
                    Some(mode == other),
                    "{cmd:?} in {other:?}"
                );
            }
            assert_eq!(cmd.unavailable(&State::default()), None);
        }
    }

    #[test]
    fn context_actions_share_definitions_and_availability() {
        assert!(context_menu().iter().any(|item| matches!(
            item,
            Item::Submenu("Selection", commands)
                if commands.contains(&Cmd::SelectAll)
                    && commands.contains(&Cmd::InvertSelection)
                    && commands.contains(&Cmd::SelectPattern)
        )));
        let commands: Vec<_> = context_menu()
            .into_iter()
            .flat_map(|item| match item {
                Item::Cmd(cmd) => vec![cmd],
                Item::OpenWith => vec![Cmd::OpenWith],
                Item::Submenu(_, commands) => commands.to_vec(),
                Item::Separator => Vec::new(),
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
            Cmd::CopyName,
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
    fn open_with_accepts_local_directories_but_not_parent_entries() {
        let directory = State {
            open_with_path: Some("/tmp/example".into()),
            ..State::default()
        };
        assert_eq!(Cmd::OpenWith.unavailable(&directory), None);
        assert_eq!(Cmd::EditFile.unavailable(&directory), Some("Select a file"));
        assert_eq!(
            Cmd::OpenWith.unavailable(&State::default()),
            Some("Select a file or directory")
        );
    }

    #[test]
    fn open_in_browser_needs_a_remote_with_a_web_view() {
        let local = State::default();
        assert_eq!(
            Cmd::OpenInBrowser.unavailable(&local),
            Some("Not a remote folder")
        );
        let remote = |web| State {
            remote: Some(Capabilities {
                web,
                ..Default::default()
            }),
            ..State::default()
        };
        assert_eq!(
            Cmd::OpenInBrowser.unavailable(&remote(false)),
            Some("This remote has no web view")
        );
        assert_eq!(Cmd::OpenInBrowser.unavailable(&remote(true)), None);
    }

    #[test]
    fn go_to_other_folder_explains_why_it_is_blocked() {
        assert_eq!(Cmd::GoToOtherFolder.unavailable(&State::default()), None);
        let places = State {
            other_folder_blocked: Some("The other panel shows Places"),
            ..State::default()
        };
        assert_eq!(
            Cmd::GoToOtherFolder.unavailable(&places),
            Some("The other panel shows Places")
        );
    }

    #[test]
    fn swap_panels_explains_why_it_is_blocked() {
        assert_eq!(Cmd::SwapPanels.unavailable(&State::default()), None);
        let busy = State {
            swap_blocked: Some("busy"),
            ..State::default()
        };
        assert_eq!(Cmd::SwapPanels.unavailable(&busy), Some("busy"));
    }

    #[test]
    fn clear_selection_needs_marks() {
        let none = State::default();
        assert_eq!(
            Cmd::ClearSelection.unavailable(&none),
            Some("Nothing is marked")
        );
        let marked = State {
            has_marks: true,
            ..none
        };
        assert_eq!(Cmd::ClearSelection.unavailable(&marked), None);
    }

    #[test]
    fn copy_name_needs_a_cursor_entry_even_with_marks() {
        assert_eq!(
            Cmd::CopyName.unavailable(&State {
                has_targets: true,
                has_marks: true,
                ..State::default()
            }),
            Some("Select a file or directory")
        );
        assert_eq!(
            Cmd::CopyName.unavailable(&State {
                cursor_on_entry: true,
                ..State::default()
            }),
            None
        );
    }

    #[test]
    fn bookmark_toggle_follows_location_and_state() {
        let folder = State {
            in_directory: true,
            ..State::default()
        };
        assert_eq!(Cmd::ToggleBookmark.unavailable(&folder), None);
        assert_eq!(Cmd::ToggleBookmark.checked(&folder), Some(false));
        let marked = State {
            bookmarked: true,
            ..folder.clone()
        };
        assert_eq!(Cmd::ToggleBookmark.checked(&marked), Some(true));
        let results = State::default();
        assert_eq!(
            Cmd::ToggleBookmark.unavailable(&results),
            Some("Not in a folder")
        );
        let archive = State {
            in_archive: true,
            ..folder.clone()
        };
        assert!(Cmd::ToggleBookmark.unavailable(&archive).is_some());
        let remote = State {
            remote: Some(Capabilities::default()),
            ..folder.clone()
        };
        assert_eq!(Cmd::ToggleBookmark.unavailable(&remote), None);
        let remote_archive = State {
            in_archive: true,
            ..remote
        };
        assert!(Cmd::ToggleBookmark.unavailable(&remote_archive).is_some());
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
        assert_eq!(
            Cmd::CalculateSizes.unavailable(&idle),
            Some("No folders to measure")
        );
        let folders = State {
            has_folders: true,
            ..idle.clone()
        };
        assert_eq!(Cmd::CalculateSizes.unavailable(&folders), None);
        let in_archive = State {
            in_archive: true,
            ..folders
        };
        assert!(Cmd::CalculateSizes.unavailable(&in_archive).is_some());

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

    #[test]
    fn remote_folders_limit_commands_to_what_the_remote_can_do() {
        let remote = |caps| State {
            has_targets: true,
            cursor_on_file: true,
            in_directory: true,
            remote: Some(caps),
            ..State::default()
        };
        let full = remote(Capabilities {
            upload: true,
            removal: Some(Removal::Trash),
            create_folder: true,
            rename: true,
            move_within: true,
            web: false,
        });
        for cmd in [
            Cmd::OpenWith,
            Cmd::EditFile,
            Cmd::NewFile,
            Cmd::FindFiles,
            Cmd::CalculateSizes,
            Cmd::OpenTerminal,
            Cmd::ToolsMenu,
            Cmd::YankFiles,
            Cmd::PasteFiles,
        ] {
            assert_eq!(
                cmd.unavailable(&full),
                Some("Not available on remote folders"),
                "{cmd:?}"
            );
        }
        for cmd in [
            Cmd::Open,
            Cmd::Refresh,
            Cmd::Rename,
            Cmd::MakeDir,
            Cmd::Delete,
            Cmd::Copy,
            Cmd::Move,
            Cmd::GoUp,
            Cmd::ToggleBookmark,
        ] {
            assert_eq!(cmd.unavailable(&full), None, "{cmd:?}");
        }
        assert!(Cmd::DeletePermanently.unavailable(&full).is_some());
        let read_only = remote(Capabilities::default());
        for cmd in [
            Cmd::Rename,
            Cmd::MakeDir,
            Cmd::Delete,
            Cmd::DeletePermanently,
        ] {
            assert!(cmd.unavailable(&read_only).is_some(), "{cmd:?}");
        }
        let permanent = remote(Capabilities {
            removal: Some(Removal::Permanent),
            ..Default::default()
        });
        assert_eq!(Cmd::Delete.unavailable(&permanent), None);
        assert!(Cmd::DeletePermanently.unavailable(&permanent).is_some());

        let blocked = State {
            has_targets: true,
            copy_blocked: Some("no copy"),
            move_blocked: Some("no move"),
            ..State::default()
        };
        assert_eq!(Cmd::Copy.unavailable(&blocked), Some("no copy"));
        assert_eq!(Cmd::Move.unavailable(&blocked), Some("no move"));
    }

    #[test]
    fn view_commands_follow_the_panel_mode() {
        let list = State {
            icon_size: 96,
            ..State::default()
        };
        assert_eq!(Cmd::ListView.checked(&list), Some(true));
        assert_eq!(Cmd::IconView.checked(&list), Some(false));
        assert_eq!(
            Cmd::LargerIcons.unavailable(&list),
            Some("Only in icon view")
        );
        assert_eq!(Cmd::ToggleTree.unavailable(&list), None);

        let icons = State {
            view: ViewMode::Icons,
            tree: false,
            ..list
        };
        assert_eq!(Cmd::IconView.checked(&icons), Some(true));
        assert_eq!(Cmd::LargerIcons.unavailable(&icons), None);
        assert_eq!(Cmd::SmallerIcons.unavailable(&icons), None);
        for cmd in [Cmd::ToggleTree, Cmd::Expand, Cmd::Collapse] {
            assert_eq!(
                cmd.unavailable(&icons),
                Some("Tree mode only applies to the list view")
            );
        }
        let largest = State {
            icon_size: 160,
            ..icons.clone()
        };
        assert!(Cmd::LargerIcons.unavailable(&largest).is_some());
        assert_eq!(Cmd::SmallerIcons.unavailable(&largest), None);
        let smallest = State {
            icon_size: 48,
            ..icons
        };
        assert!(Cmd::SmallerIcons.unavailable(&smallest).is_some());
    }
}
