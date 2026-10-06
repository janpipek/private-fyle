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
        TogglePreview,
        Filter,
        EditFile,
        NewFile,
        SelectTheme,
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
        YankFiles,
        CutFiles,
        PasteFiles,
        Help,
        Rename,
        GoTo,
        EditLocation,
        CopyLocation,
        AddBookmark,
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
        Quit,
    ]
);

pub const CONTEXT: &str = "Commander";
/// Overrides for keys the DataTable would otherwise use for column navigation.
pub const TABLE_CONTEXT: &str = "Commander > DataTable";

/// Binds the user's `[[keymap]]` entries; returns errors for bad ones.
pub fn bind_user_keys(entries: &[crate::settings::KeymapEntry], cx: &mut App) -> Vec<String> {
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
        let context = match entry.context {
            KeyContext::Panel => TABLE_CONTEXT,
            KeyContext::Global => CONTEXT,
            KeyContext::Editor => "EditorPane",
        };
        let predicate = KeyBindingContextPredicate::parse(context)
            .ok()
            .map(Into::into);
        match KeyBinding::load(
            &entry.keys,
            action,
            predicate,
            false,
            None,
            cx.keyboard_mapper().as_ref(),
        ) {
            Ok(binding) => bindings.push(binding),
            Err(e) => errors.push(format!("keymap: {:?}: {e}", entry.keys)),
        }
    }
    cx.bind_keys(bindings);
    errors
}

pub fn bind_keys(cx: &mut App) {
    // Window-wide keys; panel keys are bound in the table context so they do
    // not fire while typing in the editor.
    let ctx = Some(CONTEXT);
    let table = Some(TABLE_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("enter", Open, table),
        KeyBinding::new("shift-enter", OpenWith, table),
        KeyBinding::new("backspace", GoUp, table),
        KeyBinding::new("alt-left", GoBack, table),
        KeyBinding::new("alt-right", GoForward, table),
        KeyBinding::new("tab", SwitchPanel, table),
        KeyBinding::new("shift-tab", SwitchPanel, table),
        KeyBinding::new("home", GoFirst, table),
        KeyBinding::new("end", GoLast, table),
        KeyBinding::new("left", Collapse, table),
        KeyBinding::new("right", Expand, table),
        KeyBinding::new("escape", Unmark, table),
        KeyBinding::new("insert", ToggleMark, table),
        KeyBinding::new("space", ToggleMark, table),
        KeyBinding::new("secondary-a", SelectAll, table),
        KeyBinding::new("*", InvertSelection, table),
        KeyBinding::new("+", SelectPattern, table),
        KeyBinding::new("f1", Help, ctx),
        KeyBinding::new("f2", ToolsMenu, table),
        KeyBinding::new("secondary-c", YankFiles, table),
        KeyBinding::new("secondary-x", CutFiles, table),
        KeyBinding::new("secondary-v", PasteFiles, table),
        KeyBinding::new("shift-f6", Rename, table),
        KeyBinding::new("ctrl-g", GoTo, table),
        KeyBinding::new("alt-f1", GoTo, table),
        KeyBinding::new("secondary-l", EditLocation, table),
        KeyBinding::new("ctrl-d", AddBookmark, table),
        KeyBinding::new("f9", OpenTerminal, table),
        KeyBinding::new("alt-f7", FindFiles, table),
        KeyBinding::new("secondary-shift-f", FindFiles, table),
        KeyBinding::new("f3", TogglePreview, ctx),
        KeyBinding::new("/", Filter, table),
        KeyBinding::new("ctrl-f", Filter, table),
        KeyBinding::new("f4", EditFile, table),
        KeyBinding::new("shift-f4", NewFile, table),
        KeyBinding::new("ctrl-k ctrl-t", SelectTheme, ctx),
        KeyBinding::new("secondary-p", CommandPalette, ctx),
        KeyBinding::new("secondary-shift-p", CommandPalette, ctx),
        KeyBinding::new("f5", Copy, table),
        KeyBinding::new("shift-f5", ExtractArchive, table),
        KeyBinding::new("f6", Move, table),
        KeyBinding::new("f7", MakeDir, table),
        KeyBinding::new("f8", Delete, table),
        KeyBinding::new("delete", Delete, table),
        KeyBinding::new("shift-f8", DeletePermanently, table),
        KeyBinding::new("shift-delete", DeletePermanently, table),
        KeyBinding::new("secondary-r", Refresh, table),
        KeyBinding::new("ctrl-shift-s", CalculateSizes, table),
        KeyBinding::new("ctrl-h", ToggleHidden, table),
        KeyBinding::new("ctrl-t", ToggleTree, table),
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

pub fn action_for(cmd: Cmd) -> Box<dyn Action> {
    match cmd {
        Cmd::Open => Box::new(Open),
        Cmd::OpenWith => Box::new(OpenWith),
        Cmd::EditFile => Box::new(EditFile),
        Cmd::NewFile => Box::new(NewFile),
        Cmd::MakeDir => Box::new(MakeDir),
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
        Cmd::Filter => Box::new(Filter),
        Cmd::TogglePreview => Box::new(TogglePreview),
        Cmd::ToggleTree => Box::new(ToggleTree),
        Cmd::Expand => Box::new(Expand),
        Cmd::Collapse => Box::new(Collapse),
        Cmd::ToggleHidden => Box::new(ToggleHidden),
        Cmd::SelectColumns => Box::new(SelectColumns),
        Cmd::SelectTheme => Box::new(SelectTheme),
        Cmd::Refresh => Box::new(Refresh),
        Cmd::CalculateSizes => Box::new(CalculateSizes),
        Cmd::GoUp => Box::new(GoUp),
        Cmd::GoFirst => Box::new(GoFirst),
        Cmd::GoLast => Box::new(GoLast),
        Cmd::SwitchPanel => Box::new(SwitchPanel),
        Cmd::GoTo => Box::new(GoTo),
        Cmd::EditLocation => Box::new(EditLocation),
        Cmd::CopyLocation => Box::new(CopyLocation),
        Cmd::AddBookmark => Box::new(AddBookmark),
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
