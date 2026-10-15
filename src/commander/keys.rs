//! Actions, key bindings and the command-to-action mapping.

use gpui_kit::*;

use super::Commander;
use crate::menus::Cmd;

actions!(
    fyle,
    [
        Open,
        GoUp,
        GoFirst,
        GoLast,
        SwitchPanel,
        ToggleMark,
        Unmark,
        ClearSelection,
        Copy,
        Move,
        ExtractArchive,
        MakeDir,
        Delete,
        DeletePermanently,
        Refresh,
        CalculateSizes,
        ToggleHidden,
        ToggleTree,
        ListView,
        IconView,
        LargerIcons,
        SmallerIcons,
        TogglePreview,
        ShowProperties,
        LayoutTwoPanels,
        LayoutTwoPanelsInspector,
        LayoutOnePanelInspector,
        Filter,
        EditFile,
        NewFile,
        SelectTheme,
        ToggleCustomTitlebar,
        SelectColumns,
        SelectAll,
        InvertSelection,
        SelectPattern,
        SortByName,
        SortByExt,
        SortBySize,
        SortByModified,
        SortDescending,
        GoBack,
        GoForward,
        OpenConfig,
        About,
        OpenWith,
        OpenInBrowser,
        YankFiles,
        CutFiles,
        PasteFiles,
        Help,
        Rename,
        GoTo,
        EditLocation,
        CopyLocation,
        CopyFullPath,
        CopyName,
        ToggleBookmark,
        GoToOtherFolder,
        SwapPanels,
        OpenTerminal,
        CancelJobs,
        FindFiles,
        ToolsMenu,
        CommandPalette,
        Expand,
        Collapse,
        ZoomIn,
        ZoomOut,
        ZoomReset,
        EditShortcuts,
        ManageRemotes,
        Quit,
    ]
);

pub const CONTEXT: &str = "Commander";
/// Overrides for keys the DataTable would otherwise use for column navigation.
pub const TABLE_CONTEXT: &str = "Commander > DataTable";
/// The icon grid's equivalent of [`TABLE_CONTEXT`].
pub const GRID_CONTEXT: &str = "Commander > IconGrid";

/// The range occupied by user bindings in the app's ordered keymap.
#[derive(Default)]
struct UserBindings(Option<(usize, usize)>);

impl Global for UserBindings {}

/// Binds the user's `[[keymap]]` entries; returns errors for bad ones.
fn user_key_bindings(
    entries: &[crate::settings::KeymapEntry],
    cx: &App,
) -> (Vec<KeyBinding>, Vec<String>) {
    use crate::settings::KeyContext;
    let mut errors = Vec::new();
    let mut bindings = Vec::new();
    for entry in entries {
        let action: Box<dyn Action> = if entry.action.eq_ignore_ascii_case("none") {
            Box::new(NoAction)
        } else {
            let name = if entry.action.contains("::") {
                entry.action.clone()
            } else {
                format!("fyle::{}", entry.action)
            };
            match cx.build_action(&name, None) {
                Ok(action) => action,
                Err(_) => {
                    errors.push(format!("keymap: unknown action {:?}", entry.action));
                    continue;
                }
            }
        };
        // Panel keys apply in the list and in the icon grid.
        let contexts: &[&str] = match entry.context {
            KeyContext::Panel => &[TABLE_CONTEXT, GRID_CONTEXT],
            KeyContext::Global => &[CONTEXT],
            KeyContext::Editor => &["EditorPane"],
        };
        for context in contexts {
            // The built-in quit shortcut has no context so it also works in dialogs.
            // A Commander-scoped NoAction cannot suppress that binding.
            let predicate = if entry.context == KeyContext::Global
                && entry.action.eq_ignore_ascii_case("none")
                && entry.keys.eq_ignore_ascii_case("secondary-q")
            {
                None
            } else {
                KeyBindingContextPredicate::parse(context)
                    .ok()
                    .map(Into::into)
            };
            match KeyBinding::load(
                &entry.keys,
                action.boxed_clone(),
                predicate,
                false,
                None,
                cx.keyboard_mapper().as_ref(),
            ) {
                Ok(binding) => bindings.push(binding),
                Err(e) => {
                    errors.push(format!("keymap: {:?}: {e}", entry.keys));
                    break;
                }
            }
        }
    }
    (bindings, errors)
}

/// Checks user key entries against GPUI's action registry and keystroke parser.
pub fn validate_user_keys(
    entries: &[crate::settings::KeymapEntry],
    cx: &App,
) -> Result<(), Vec<String>> {
    let (_, errors) = user_key_bindings(entries, cx);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Installs the user's key entries, replacing any entries installed earlier.
pub fn replace_user_keys(entries: &[crate::settings::KeymapEntry], cx: &mut App) -> Vec<String> {
    let (new_bindings, errors) = user_key_bindings(entries, cx);
    let previous_range = cx
        .try_global::<UserBindings>()
        .and_then(|previous| previous.0);
    let current: Vec<_> = cx.key_bindings().borrow().bindings().cloned().collect();
    // Bindings registered after the user range append to the keymap, so the
    // original range remains stable even when views register keys later.
    let (start, count) = previous_range.unwrap_or((current.len(), 0));
    let mut retained: Vec<_> = current
        .into_iter()
        .enumerate()
        .filter_map(|(index, binding)| {
            (!(start <= index && index < start + count)).then_some(binding)
        })
        .collect();
    let new_start = retained.len();
    let new_count = new_bindings.len();
    retained.extend(new_bindings.iter().cloned());
    cx.clear_key_bindings();
    cx.bind_keys(retained);
    cx.set_global(UserBindings(Some((new_start, new_count))));
    errors
}

/// Binds user keys during startup, preserving the runtime replacement handle.
pub fn bind_user_keys(entries: &[crate::settings::KeymapEntry], cx: &mut App) -> Vec<String> {
    replace_user_keys(entries, cx)
}

/// Keys for the panel's key context: the list (`Commander > DataTable`) or
/// the icon grid (`Commander > IconGrid`). `tree_keys` adds Left / Right as
/// collapse / expand; the grid uses those keys to move between cells.
fn panel_bindings(context: &'static str, tree_keys: bool) -> Vec<KeyBinding> {
    let context = Some(context);
    let mut bindings = vec![
        KeyBinding::new("enter", Open, context),
        KeyBinding::new("shift-enter", OpenWith, context),
        KeyBinding::new("backspace", GoUp, context),
        KeyBinding::new("alt-left", GoBack, context),
        KeyBinding::new("alt-right", GoForward, context),
        KeyBinding::new("tab", SwitchPanel, context),
        KeyBinding::new("shift-tab", SwitchPanel, context),
        KeyBinding::new("home", GoFirst, context),
        KeyBinding::new("end", GoLast, context),
        KeyBinding::new("escape", Unmark, context),
        KeyBinding::new("insert", ToggleMark, context),
        KeyBinding::new("space", ToggleMark, context),
        KeyBinding::new("secondary-a", SelectAll, context),
        KeyBinding::new("*", InvertSelection, context),
        KeyBinding::new("+", SelectPattern, context),
        KeyBinding::new("f2", ToolsMenu, context),
        KeyBinding::new("secondary-c", YankFiles, context),
        KeyBinding::new("secondary-x", CutFiles, context),
        KeyBinding::new("secondary-v", PasteFiles, context),
        KeyBinding::new("shift-f6", Rename, context),
        KeyBinding::new("ctrl-g", GoTo, context),
        KeyBinding::new("alt-f1", GoTo, context),
        KeyBinding::new("secondary-l", EditLocation, context),
        KeyBinding::new("ctrl-d", ToggleBookmark, context),
        KeyBinding::new("ctrl-u", SwapPanels, context),
        KeyBinding::new("ctrl-left", GoToOtherFolder, context),
        KeyBinding::new("ctrl-right", GoToOtherFolder, context),
        KeyBinding::new("f9", OpenTerminal, context),
        KeyBinding::new("alt-f7", FindFiles, context),
        KeyBinding::new("secondary-shift-f", FindFiles, context),
        KeyBinding::new("/", Filter, context),
        KeyBinding::new("ctrl-f", Filter, context),
        KeyBinding::new("f4", EditFile, context),
        KeyBinding::new("shift-f4", NewFile, context),
        KeyBinding::new("f5", Copy, context),
        KeyBinding::new("shift-f5", ExtractArchive, context),
        KeyBinding::new("f6", Move, context),
        KeyBinding::new("f7", MakeDir, context),
        KeyBinding::new("f8", Delete, context),
        KeyBinding::new("delete", Delete, context),
        KeyBinding::new("shift-f8", DeletePermanently, context),
        KeyBinding::new("shift-delete", DeletePermanently, context),
        KeyBinding::new("secondary-r", Refresh, context),
        KeyBinding::new("ctrl-shift-s", CalculateSizes, context),
        KeyBinding::new("ctrl-h", ToggleHidden, context),
        KeyBinding::new("ctrl-t", ToggleTree, context),
        KeyBinding::new("ctrl-1", ListView, context),
        KeyBinding::new("ctrl-2", IconView, context),
        KeyBinding::new("alt-=", LargerIcons, context),
        KeyBinding::new("alt-+", LargerIcons, context),
        KeyBinding::new("alt--", SmallerIcons, context),
    ];
    if tree_keys {
        bindings.push(KeyBinding::new("left", Collapse, context));
        bindings.push(KeyBinding::new("right", Expand, context));
    }
    bindings
}

pub fn bind_keys(cx: &mut App) {
    // Window-wide keys; panel keys are bound in the table context so they do
    // not fire while typing in the editor.
    let ctx = Some(CONTEXT);
    cx.bind_keys(panel_bindings(TABLE_CONTEXT, true));
    cx.bind_keys(panel_bindings(GRID_CONTEXT, false));
    cx.bind_keys([
        KeyBinding::new("f1", Help, ctx),
        KeyBinding::new("f3", TogglePreview, ctx),
        KeyBinding::new("alt-enter", ShowProperties, ctx),
        KeyBinding::new("ctrl-k ctrl-t", SelectTheme, ctx),
        KeyBinding::new("secondary-p", CommandPalette, ctx),
        KeyBinding::new("secondary-shift-p", CommandPalette, ctx),
        KeyBinding::new("secondary-=", ZoomIn, ctx),
        KeyBinding::new("secondary-+", ZoomIn, ctx),
        KeyBinding::new("secondary-shift-=", ZoomIn, ctx),
        KeyBinding::new("secondary--", ZoomOut, ctx),
        KeyBinding::new("secondary-0", ZoomReset, ctx),
        KeyBinding::new("f10", Quit, ctx),
        KeyBinding::new("secondary-q", Quit, None),
        KeyBinding::new("ctrl-shift-c", CancelJobs, ctx),
    ]);
}

/// The gpui action behind a [`Cmd`].
pub fn action_for(cmd: Cmd) -> Box<dyn Action> {
    match cmd {
        Cmd::Open => Box::new(Open),
        Cmd::OpenWith => Box::new(OpenWith),
        Cmd::EditFile => Box::new(EditFile),
        Cmd::NewFile => Box::new(NewFile),
        Cmd::MakeDir => Box::new(MakeDir),
        Cmd::OpenInBrowser => Box::new(OpenInBrowser),
        Cmd::Rename => Box::new(Rename),
        Cmd::Copy => Box::new(Copy),
        Cmd::Move => Box::new(Move),
        Cmd::ExtractArchive => Box::new(ExtractArchive),
        Cmd::Delete => Box::new(Delete),
        Cmd::DeletePermanently => Box::new(DeletePermanently),
        Cmd::Quit => Box::new(Quit),
        Cmd::CutFiles => Box::new(CutFiles),
        Cmd::YankFiles => Box::new(YankFiles),
        Cmd::PasteFiles => Box::new(PasteFiles),
        Cmd::ToggleMark => Box::new(ToggleMark),
        Cmd::Unmark => Box::new(Unmark),
        Cmd::ClearSelection => Box::new(ClearSelection),
        Cmd::Filter => Box::new(Filter),
        Cmd::TogglePreview => Box::new(TogglePreview),
        Cmd::ShowProperties => Box::new(ShowProperties),
        Cmd::LayoutTwoPanels => Box::new(LayoutTwoPanels),
        Cmd::LayoutTwoPanelsInspector => Box::new(LayoutTwoPanelsInspector),
        Cmd::LayoutOnePanelInspector => Box::new(LayoutOnePanelInspector),
        Cmd::ToggleTree => Box::new(ToggleTree),
        Cmd::ListView => Box::new(ListView),
        Cmd::IconView => Box::new(IconView),
        Cmd::LargerIcons => Box::new(LargerIcons),
        Cmd::SmallerIcons => Box::new(SmallerIcons),
        Cmd::Expand => Box::new(Expand),
        Cmd::Collapse => Box::new(Collapse),
        Cmd::ToggleHidden => Box::new(ToggleHidden),
        Cmd::SelectColumns => Box::new(SelectColumns),
        Cmd::SelectTheme => Box::new(SelectTheme),
        Cmd::ToggleCustomTitlebar => Box::new(ToggleCustomTitlebar),
        Cmd::Refresh => Box::new(Refresh),
        Cmd::CalculateSizes => Box::new(CalculateSizes),
        Cmd::GoUp => Box::new(GoUp),
        Cmd::GoFirst => Box::new(GoFirst),
        Cmd::GoLast => Box::new(GoLast),
        Cmd::SwitchPanel => Box::new(SwitchPanel),
        Cmd::GoTo => Box::new(GoTo),
        Cmd::EditLocation => Box::new(EditLocation),
        Cmd::CopyLocation => Box::new(CopyLocation),
        Cmd::CopyFullPath => Box::new(CopyFullPath),
        Cmd::CopyName => Box::new(CopyName),
        Cmd::ToggleBookmark => Box::new(ToggleBookmark),
        Cmd::GoToOtherFolder => Box::new(GoToOtherFolder),
        Cmd::SwapPanels => Box::new(SwapPanels),
        Cmd::OpenTerminal => Box::new(OpenTerminal),
        Cmd::ToolsMenu => Box::new(ToolsMenu),
        Cmd::FindFiles => Box::new(FindFiles),
        Cmd::CancelJobs => Box::new(CancelJobs),
        Cmd::CommandPalette => Box::new(CommandPalette),
        Cmd::Help => Box::new(Help),
        Cmd::SelectAll => Box::new(SelectAll),
        Cmd::InvertSelection => Box::new(InvertSelection),
        Cmd::SelectPattern => Box::new(SelectPattern),
        Cmd::SortByName => Box::new(SortByName),
        Cmd::SortByExt => Box::new(SortByExt),
        Cmd::SortBySize => Box::new(SortBySize),
        Cmd::SortByModified => Box::new(SortByModified),
        Cmd::SortDescending => Box::new(SortDescending),
        Cmd::GoBack => Box::new(GoBack),
        Cmd::GoForward => Box::new(GoForward),
        Cmd::OpenConfig => Box::new(OpenConfig),
        Cmd::About => Box::new(About),
        Cmd::ZoomIn => Box::new(ZoomIn),
        Cmd::ZoomOut => Box::new(ZoomOut),
        Cmd::ZoomReset => Box::new(ZoomReset),
        Cmd::EditShortcuts => Box::new(EditShortcuts),
        Cmd::ManageRemotes => Box::new(ManageRemotes),
    }
}

impl Commander {
    /// Every command offered in the palette.
    pub fn palette_commands() -> Vec<(&'static str, Box<dyn Action>)> {
        Cmd::ALL
            .into_iter()
            .map(|cmd| (cmd.palette_label(), action_for(cmd)))
            .collect()
    }
}
