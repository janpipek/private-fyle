//! UI-independent editing and validation for user key bindings.

use crate::menus::Cmd;
use crate::settings::{KeyContext, KeymapEntry};

/// A command available to the keymap editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandBinding {
    /// Stable action ID used in config.toml.
    pub action: &'static str,
    /// Name shown to users.
    pub label: &'static str,
    /// Default shortcut, if the command has one.
    pub keys: &'static str,
    /// Context for its default shortcut.
    pub context: KeyContext,
}

/// Why a key binding could not be added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The action ID is not a supported command.
    UnknownAction(String),
    /// The keystroke string is empty or malformed.
    InvalidKeys(String),
    /// Another binding uses this keystroke in an overlapping context.
    Conflict {
        keys: String,
        action: String,
        context: KeyContext,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownAction(action) => write!(f, "unknown command {action:?}"),
            Self::InvalidKeys(keys) => write!(f, "invalid shortcut {keys:?}"),
            Self::Conflict {
                keys,
                action,
                context,
            } => {
                write!(f, "{keys:?} is already assigned to {action} in {context:?}")
            }
        }
    }
}

impl std::error::Error for Error {}

/// Editable user overrides. Defaults remain in the runtime key bindings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Editor {
    entries: Vec<KeymapEntry>,
}

impl Editor {
    /// Creates an editor from the saved overrides.
    pub fn new(entries: Vec<KeymapEntry>) -> Self {
        Self { entries }
    }

    /// Returns the overrides in their config order.
    pub fn entries(&self) -> &[KeymapEntry] {
        &self.entries
    }

    /// Replaces an action's override across contexts after validation.
    pub fn set(&mut self, action: &str, keys: &str, context: KeyContext) -> Result<(), Error> {
        if !commands().iter().any(|command| command.action == action) {
            return Err(Error::UnknownAction(action.to_owned()));
        }
        let normalized = normalize_keys(keys).ok_or_else(|| Error::InvalidKeys(keys.to_owned()))?;
        let catalog = commands();
        let default_conflict = catalog.iter().find_map(|command| {
            if command.action == action || !contexts_overlap(command.context, context) {
                return None;
            }
            let active_key = default_keys_in(command.action, command.context)
                .into_iter()
                .any(|key| {
                    normalize_keys(key).as_deref() == Some(&normalized)
                        && !self.entries.iter().any(|entry| {
                            entry.action == "none"
                                && entry.context == command.context
                                && normalize_keys(&entry.keys).as_deref() == Some(&normalized)
                        })
                });
            active_key.then_some(*command)
        });
        if let Some(default) = default_conflict {
            return Err(Error::Conflict {
                keys: keys.to_owned(),
                action: default.action.to_owned(),
                context: default.context,
            });
        }
        if let Some(conflict) = self.entries.iter().find(|entry| {
            entry.action != action
                && entry.action != "none"
                && contexts_overlap(entry.context, context)
                && normalize_keys(&entry.keys).as_deref() == Some(&normalized)
        }) {
            return Err(Error::Conflict {
                keys: keys.to_owned(),
                action: conflict.action.clone(),
                context: conflict.context,
            });
        }
        let default_binding = catalog
            .iter()
            .find(|command| command.action == action && command.context == context)
            .or_else(|| catalog.iter().find(|command| command.action == action));
        let default_context = default_binding.map(|command| command.context);
        self.remove_action_overrides(action);
        if let Some(default_context) = default_context {
            for default_key in default_keys_in(action, default_context) {
                if context != default_context
                    || normalize_keys(default_key).as_deref() != Some(&normalized)
                {
                    self.entries.push(KeymapEntry {
                        keys: default_key.to_owned(),
                        action: "none".to_owned(),
                        context: default_context,
                    });
                }
            }
        }
        self.entries.push(KeymapEntry {
            keys: keys.trim().to_owned(),
            action: action.to_owned(),
            context,
        });
        Ok(())
    }

    fn remove_action_overrides(&mut self, action: &str) {
        self.entries.retain(|entry| entry.action != action);
        let Some(context) = commands()
            .into_iter()
            .find(|command| command.action == action)
            .map(|command| command.context)
        else {
            return;
        };
        for key in default_keys_in(action, context) {
            let default = normalize_keys(key);
            self.entries.retain(|entry| {
                !(entry.action == "none"
                    && entry.context == context
                    && normalize_keys(&entry.keys) == default)
            });
        }
    }

    /// Disables a command across contexts, including all default shortcuts.
    pub fn disable(&mut self, action: &str, _context: KeyContext) {
        if !commands().iter().any(|command| command.action == action) {
            return;
        }
        self.remove_action_overrides(action);
        let Some(default_context) = commands()
            .into_iter()
            .find(|command| command.action == action)
            .map(|command| command.context)
        else {
            return;
        };
        for key in default_keys_in(action, default_context) {
            self.entries.push(KeymapEntry {
                keys: key.to_owned(),
                action: "none".to_owned(),
                context: default_context,
            });
        }
    }

    /// Resets one command's overrides across contexts, restoring its defaults.
    pub fn reset(&mut self, action: &str, context: KeyContext) {
        let _ = context;
        self.remove_action_overrides(action);
    }

    /// Removes all overrides, restoring the default keymap.
    pub fn reset_all(&mut self) {
        self.entries.clear();
    }
}

/// Lists commands supported by the editor.
pub fn commands() -> Vec<CommandBinding> {
    Cmd::ALL
        .iter()
        .copied()
        .map(|cmd| {
            let action = action_id(cmd);
            let (keys, context) = default_binding(cmd);
            CommandBinding {
                action,
                label: cmd.label(),
                keys,
                context,
            }
        })
        .collect()
}

fn action_id(cmd: Cmd) -> &'static str {
    match cmd {
        Cmd::Open => "Open",
        Cmd::OpenWith => "OpenWith",
        Cmd::OpenInBrowser => "OpenInBrowser",
        Cmd::EditFile => "EditFile",
        Cmd::NewFile => "NewFile",
        Cmd::MakeDir => "MakeDir",
        Cmd::Rename => "Rename",
        Cmd::Copy => "Copy",
        Cmd::Move => "Move",
        Cmd::ExtractArchive => "ExtractArchive",
        Cmd::Delete => "Delete",
        Cmd::DeletePermanently => "DeletePermanently",
        Cmd::Quit => "Quit",
        Cmd::CutFiles => "CutFiles",
        Cmd::YankFiles => "YankFiles",
        Cmd::PasteFiles => "PasteFiles",
        Cmd::ToggleMark => "ToggleMark",
        Cmd::SelectAll => "SelectAll",
        Cmd::InvertSelection => "InvertSelection",
        Cmd::SelectPattern => "SelectPattern",
        Cmd::Unmark => "Unmark",
        Cmd::ClearSelection => "ClearSelection",
        Cmd::Filter => "Filter",
        Cmd::TogglePreview => "TogglePreview",
        Cmd::LayoutTwoPanels => "LayoutTwoPanels",
        Cmd::LayoutTwoPanelsInspector => "LayoutTwoPanelsInspector",
        Cmd::LayoutOnePanelInspector => "LayoutOnePanelInspector",
        Cmd::ToggleTree => "ToggleTree",
        Cmd::ListView => "ListView",
        Cmd::IconView => "IconView",
        Cmd::LargerIcons => "LargerIcons",
        Cmd::SmallerIcons => "SmallerIcons",
        Cmd::Expand => "Expand",
        Cmd::Collapse => "Collapse",
        Cmd::ToggleHidden => "ToggleHidden",
        Cmd::SortByName => "SortByName",
        Cmd::SortByExt => "SortByExt",
        Cmd::SortBySize => "SortBySize",
        Cmd::SortByModified => "SortByModified",
        Cmd::SortDescending => "SortDescending",
        Cmd::SelectColumns => "SelectColumns",
        Cmd::SelectTheme => "SelectTheme",
        Cmd::Refresh => "Refresh",
        Cmd::CalculateSizes => "CalculateSizes",
        Cmd::GoBack => "GoBack",
        Cmd::GoForward => "GoForward",
        Cmd::GoUp => "GoUp",
        Cmd::GoFirst => "GoFirst",
        Cmd::GoLast => "GoLast",
        Cmd::SwitchPanel => "SwitchPanel",
        Cmd::EditLocation => "EditLocation",
        Cmd::CopyLocation => "CopyLocation",
        Cmd::GoTo => "GoTo",
        Cmd::GoToOtherFolder => "GoToOtherFolder",
        Cmd::SwapPanels => "SwapPanels",
        Cmd::ToggleBookmark => "ToggleBookmark",
        Cmd::OpenTerminal => "OpenTerminal",
        Cmd::ToolsMenu => "ToolsMenu",
        Cmd::FindFiles => "FindFiles",
        Cmd::CancelJobs => "CancelJobs",
        Cmd::CommandPalette => "CommandPalette",
        Cmd::EditShortcuts => "EditShortcuts",
        Cmd::Help => "Help",
        Cmd::OpenConfig => "OpenConfig",
        Cmd::About => "About",
        Cmd::ZoomIn => "ZoomIn",
        Cmd::ZoomOut => "ZoomOut",
        Cmd::ZoomReset => "ZoomReset",
    }
}

fn default_binding(cmd: Cmd) -> (&'static str, KeyContext) {
    let panel = KeyContext::Panel;
    let global = KeyContext::Global;
    match cmd {
        Cmd::Open => ("enter", panel),
        Cmd::OpenWith => ("shift-enter", panel),
        Cmd::GoBack => ("alt-left", panel),
        Cmd::GoForward => ("alt-right", panel),
        Cmd::GoUp => ("backspace", panel),
        Cmd::GoFirst => ("home", panel),
        Cmd::GoLast => ("end", panel),
        Cmd::SwitchPanel => ("tab", panel),
        Cmd::ToggleMark => ("space", panel),
        Cmd::SelectAll => ("secondary-a", panel),
        Cmd::InvertSelection => ("*", panel),
        Cmd::SelectPattern => ("+", panel),
        Cmd::Unmark => ("escape", panel),
        Cmd::ToolsMenu => ("f2", panel),
        Cmd::YankFiles => ("secondary-c", panel),
        Cmd::CutFiles => ("secondary-x", panel),
        Cmd::PasteFiles => ("secondary-v", panel),
        Cmd::Rename => ("shift-f6", panel),
        Cmd::GoTo => ("ctrl-g", panel),
        Cmd::EditLocation => ("secondary-l", panel),
        Cmd::ToggleBookmark => ("ctrl-d", panel),
        Cmd::SwapPanels => ("ctrl-u", panel),
        Cmd::GoToOtherFolder => ("ctrl-left", panel),
        Cmd::OpenTerminal => ("f9", panel),
        Cmd::FindFiles => ("alt-f7", panel),
        Cmd::Filter => ("/", panel),
        Cmd::EditFile => ("f4", panel),
        Cmd::NewFile => ("shift-f4", panel),
        Cmd::Copy => ("f5", panel),
        Cmd::ExtractArchive => ("shift-f5", panel),
        Cmd::Move => ("f6", panel),
        Cmd::MakeDir => ("f7", panel),
        Cmd::Delete => ("f8", panel),
        Cmd::DeletePermanently => ("shift-f8", panel),
        Cmd::Refresh => ("secondary-r", panel),
        Cmd::CalculateSizes => ("ctrl-shift-s", panel),
        Cmd::ToggleHidden => ("ctrl-h", panel),
        Cmd::ToggleTree => ("ctrl-t", panel),
        Cmd::ListView => ("ctrl-1", panel),
        Cmd::IconView => ("ctrl-2", panel),
        Cmd::TogglePreview => ("f3", global),
        Cmd::SelectTheme => ("ctrl-k ctrl-t", global),
        Cmd::CommandPalette => ("secondary-p", global),
        Cmd::ZoomIn => ("secondary-=", global),
        Cmd::ZoomOut => ("secondary--", global),
        Cmd::ZoomReset => ("secondary-0", global),
        Cmd::Quit => ("f10", global),
        Cmd::Help => ("f1", global),
        Cmd::CancelJobs => ("ctrl-shift-c", global),
        _ => ("", panel),
    }
}

fn default_keys(action: &str) -> Vec<&'static str> {
    let Some(cmd) = Cmd::ALL
        .iter()
        .copied()
        .find(|cmd| action_id(*cmd) == action)
    else {
        return Vec::new();
    };
    let mut keys = vec![default_binding(cmd).0];
    match cmd {
        Cmd::SwitchPanel => keys.push("shift-tab"),
        Cmd::ToggleMark => keys.push("insert"),
        Cmd::Filter => keys.push("ctrl-f"),
        Cmd::GoTo => keys.push("alt-f1"),
        Cmd::GoToOtherFolder => keys.push("ctrl-right"),
        Cmd::FindFiles => keys.push("secondary-shift-f"),
        Cmd::Delete => keys.push("delete"),
        Cmd::DeletePermanently => keys.push("shift-delete"),
        Cmd::LargerIcons => keys.push("alt-+"),
        Cmd::CommandPalette => keys.push("secondary-shift-p"),
        Cmd::ZoomIn => {
            keys.push("secondary-+");
            keys.push("secondary-shift-=");
        }
        Cmd::Quit => keys.push("secondary-q"),
        _ => {}
    }
    keys.into_iter().filter(|key| !key.is_empty()).collect()
}

fn default_keys_in(action: &str, context: KeyContext) -> Vec<&'static str> {
    let Some(command) = commands()
        .into_iter()
        .find(|command| command.action == action)
    else {
        return Vec::new();
    };
    if command.context != context {
        return Vec::new();
    }
    default_keys(action)
}

fn contexts_overlap(a: KeyContext, b: KeyContext) -> bool {
    a == b || a == KeyContext::Global || b == KeyContext::Global
}

fn normalize_keys(keys: &str) -> Option<String> {
    let tokens: Vec<String> = keys
        .split_whitespace()
        .map(|token| token.to_ascii_lowercase())
        .collect();
    if tokens.is_empty() {
        return None;
    }
    for token in &tokens {
        if token == "alt--" || token == "shift--" || token == "ctrl--" || token == "secondary--" {
            continue;
        }
        let parts: Vec<&str> = token.split('-').collect();
        if parts.iter().any(|part| part.is_empty()) || parts.len() > 5 {
            return None;
        }
        let key = parts.last()?;
        if key
            .chars()
            .any(|ch| !(ch.is_ascii_alphanumeric() || "_*+/=.-".contains(ch)))
        {
            return None;
        }
        if parts[..parts.len() - 1].iter().any(|modifier| {
            !["ctrl", "alt", "shift", "meta", "cmd", "secondary"].contains(modifier)
        }) {
            return None;
        }
    }
    let mut normalized = Vec::with_capacity(tokens.len());
    for token in tokens {
        let parts = token.split('-').collect::<Vec<_>>();
        if parts.len() < 2 || token.ends_with("--") {
            normalized.push(token);
            continue;
        }
        let mut modifiers = parts[..parts.len() - 1]
            .iter()
            .map(|modifier| match *modifier {
                "secondary" => {
                    if cfg!(target_os = "macos") {
                        "meta"
                    } else {
                        "ctrl"
                    }
                }
                "cmd" => "meta",
                other => other,
            })
            .collect::<Vec<_>>();
        modifiers.sort_unstable();
        modifiers.push(parts[parts.len() - 1]);
        normalized.push(modifiers.join("-"));
    }
    Some(normalized.join(" "))
}

#[cfg(test)]
mod tests {
    use super::{Editor, Error, commands};
    use crate::settings::{KeyContext, KeymapEntry};

    #[test]
    fn changing_default_disables_old_key_and_reset_restores_it() {
        let mut editor = Editor::default();
        editor
            .set("Copy", "ctrl-alt-c", KeyContext::Panel)
            .expect("valid binding");
        assert!(
            editor
                .entries()
                .iter()
                .any(|entry| entry.action == "none" && entry.keys == "f5")
        );
        editor.reset("Copy", KeyContext::Panel);
        assert!(editor.entries().is_empty());
        assert!(commands().iter().any(|command| {
            command.action == "Copy" && command.context == KeyContext::Panel && command.keys == "f5"
        }));
    }

    #[test]
    fn set_replaces_action_bindings_when_context_changes() {
        let mut editor = Editor::new(vec![
            KeymapEntry {
                keys: "ctrl-x".into(),
                action: "Copy".into(),
                context: KeyContext::Panel,
            },
            KeymapEntry {
                keys: "ctrl-y".into(),
                action: "Copy".into(),
                context: KeyContext::Editor,
            },
        ]);
        editor
            .set("Copy", "ctrl-z", KeyContext::Global)
            .expect("valid shortcut");
        assert_eq!(
            editor
                .entries()
                .iter()
                .filter(|entry| entry.action == "Copy")
                .count(),
            1
        );
        assert!(editor.entries().iter().any(|entry| entry.action == "none"
            && entry.keys == "f5"
            && entry.context == KeyContext::Panel));
        assert!(editor.entries().iter().any(|entry| entry.action == "Copy"
            && entry.keys == "ctrl-z"
            && entry.context == KeyContext::Global));
        editor.reset("Copy", KeyContext::Global);
        assert!(editor.entries().is_empty());
    }

    #[test]
    fn disable_removes_custom_bindings_in_every_context() {
        let mut editor = Editor::new(vec![
            KeymapEntry {
                keys: "ctrl-x".into(),
                action: "Copy".into(),
                context: KeyContext::Panel,
            },
            KeymapEntry {
                keys: "ctrl-y".into(),
                action: "Copy".into(),
                context: KeyContext::Editor,
            },
        ]);
        editor.disable("Copy", KeyContext::Editor);
        assert!(editor.entries().iter().all(|entry| entry.action == "none"));
        assert!(
            editor
                .entries()
                .iter()
                .any(|entry| entry.keys == "f5" && entry.context == KeyContext::Panel)
        );
        editor.reset("Copy", KeyContext::Panel);
        assert!(editor.entries().is_empty());
    }

    #[test]
    fn disable_then_reset_clears_every_default_alias() {
        let mut editor = Editor::default();
        editor.disable("Delete", KeyContext::Panel);
        assert!(
            editor
                .entries()
                .iter()
                .any(|entry| entry.keys == "f8" && entry.action == "none")
        );
        assert!(
            editor
                .entries()
                .iter()
                .any(|entry| entry.keys == "delete" && entry.action == "none")
        );
        editor.reset("Delete", KeyContext::Panel);
        assert!(editor.entries().is_empty());
    }

    #[test]
    fn conflict_checks_overlapping_contexts_and_allows_disjoint_ones() {
        let mut editor = Editor::new(vec![KeymapEntry {
            keys: "ctrl-alt-x".into(),
            action: "TestAction".into(),
            context: KeyContext::Editor,
        }]);
        assert!(matches!(
            editor.set("Copy", "ctrl-alt-x", KeyContext::Panel),
            Ok(())
        ));
        assert!(matches!(
            editor.set("Move", "ctrl-alt-x", KeyContext::Global),
            Err(Error::Conflict { .. })
        ));
    }

    #[test]
    fn command_catalog_covers_every_command() {
        assert_eq!(commands().len(), crate::menus::Cmd::ALL.len());
        assert!(commands().iter().any(|command| command.action == "Copy"));
    }

    #[test]
    fn accepts_trailing_hyphen_shortcut() {
        let mut editor = Editor::default();
        editor
            .set("LargerIcons", "alt--", KeyContext::Panel)
            .expect("valid binding");
    }

    #[test]
    fn ctrl_conflicts_with_secondary_default() {
        if cfg!(target_os = "macos") {
            return;
        }
        let mut editor = Editor::default();
        assert!(matches!(
            editor.set("OpenWith", "ctrl-c", KeyContext::Panel),
            Err(Error::Conflict { action, .. }) if action == "YankFiles"
        ));
    }

    #[test]
    fn aliases_conflict_and_disabled_default_key_is_available() {
        let mut editor = Editor::default();
        assert!(matches!(
            editor.set("Copy", "delete", KeyContext::Panel),
            Err(Error::Conflict { action, .. }) if action == "Delete"
        ));
        editor.disable("Delete", KeyContext::Panel);
        assert!(editor.set("Copy", "delete", KeyContext::Panel).is_ok());
    }
}
