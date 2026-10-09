//! Menu bar state, context menus, help and the command palette.

use std::rc::Rc;

use gpui_kit::component::list::{List, ListState};
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::{IndexPath, WindowExt as _};
use gpui_kit::*;

use super::Commander;
use super::keys::action_for;
use super::keys::{CommandPalette, Help};
use super::tools::tool_menu_footer;
use crate::archive;
use crate::bookmarks;
use crate::menu_bar::MenuBuilder;
use crate::menus::{self, Cmd};
use crate::palette::{Command, Palette};
use crate::palette_view::PaletteDelegate;
use crate::remote::jobs::{Side, plan_transfer};

pub fn command_item(cmd: Cmd, state: &menus::State) -> PopupMenuItem {
    let unavailable = cmd.unavailable(state);
    let label = unavailable
        .map(|reason| format!("{} — {reason}", cmd.label()))
        .unwrap_or_else(|| cmd.label().to_string());
    PopupMenuItem::new(label)
        .action(action_for(cmd))
        .checked(cmd.checked(state).unwrap_or(false))
        .disabled(unavailable.is_some())
}

pub fn command_menu(
    mut menu: PopupMenu,
    items: &[menus::Item],
    state: &menus::State,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
    open_with: Option<(
        Vec<crate::apps::Choice>,
        std::path::PathBuf,
        WeakEntity<Commander>,
    )>,
) -> PopupMenu {
    for item in items {
        menu = match item {
            menus::Item::Cmd(cmd) => menu.item(command_item(*cmd, state)),
            menus::Item::OpenWith => match &open_with {
                Some((choices, path, commander)) => {
                    crate::commander::open_with::add_choice_submenu(
                        menu,
                        choices.clone(),
                        path.clone(),
                        commander.clone(),
                        window,
                        cx,
                    )
                }
                None => menu.item(command_item(Cmd::OpenWith, state)),
            },
            menus::Item::Separator => menu.separator(),
            menus::Item::Submenu(label, commands) => {
                let commands = commands.to_vec();
                let state = state.clone();
                menu.submenu(*label, window, cx, move |menu, _, _| {
                    commands
                        .iter()
                        .fold(menu, |menu, cmd| menu.item(command_item(*cmd, &state)))
                })
            }
        };
    }
    menu
}

impl Commander {
    /// What the menus' availability and check marks depend on.
    pub fn menu_state(&self, cx: &App) -> menus::State {
        let folder = self.folders[self.active].read(cx);
        let current = folder.current();
        let view = self.views[self.active].read(cx);
        let (from, to) = self.panel_sides(cx);
        let blocked = |copy| {
            (from != Side::Local || to != Side::Local)
                .then(|| plan_transfer(copy, &from, &to, |name| self.remote_info(name)).err())
                .flatten()
        };
        menus::State {
            swap_blocked: self.swap_blocked(cx),
            other_folder_blocked: self.other_panel_target(cx).err(),
            has_marks: folder.marked_count() > 0,
            has_targets: folder.marked_count() > 0 || current.is_some_and(|e| !e.is_parent),
            cursor_on_file: current.is_some_and(|e| !e.is_dir),
            open_with_path: current.filter(|entry| !entry.is_parent).and_then(|entry| {
                entry
                    .location
                    .as_local_path()
                    .map(std::path::Path::to_path_buf)
            }),
            can_paste: self.yank.is_some(),
            in_directory: folder.results().is_none() && !folder.path().as_os_str().is_empty(),
            bookmarked: folder
                .bookmark_target()
                .is_some_and(|dir| bookmarks::contains(&self.bookmarks, dir)),
            in_archive: folder.archive().is_some(),
            has_folders: !folder.size_targets().is_empty(),
            cursor_on_archive: folder.archive().is_none()
                && current.is_some_and(|e| {
                    !e.is_dir && !e.is_parent && archive::format_of(&e.path).is_some()
                }),
            tree: folder.is_tree(),
            view: view.view_mode(),
            icon_size: view.icon_size(cx),
            hidden: self.show_hidden,
            preview: self.workspace.mode().has_inspector(),
            workspace_layout: self.workspace.mode(),
            jobs_running: !self.jobs.is_empty(),
            sort: folder.sort(),
            can_go_back: folder.can_go_back(),
            can_go_forward: folder.can_go_forward(),
            remote: folder.is_remote().then(|| match &from {
                Side::Remote(name) => self
                    .remote_info(name)
                    .map(|info| info.capabilities)
                    .unwrap_or_default(),
                Side::Local => Default::default(),
            }),
            copy_blocked: blocked(true),
            move_blocked: blocked(false),
        }
    }

    /// Rebuilds the menu bar when what it shows changed.
    pub fn update_menu_bar(&mut self, cx: &mut Context<Self>) {
        let state = self.menu_state(cx);
        if self.menu_state.as_ref() == Some(&state) {
            return;
        }
        let commander = cx.weak_entity();
        let open_with = self
            .open_with_menu_data(cx)
            .map(|(path, choices)| (choices, path, commander.clone()));
        let menus = menus::menu_bar()
            .into_iter()
            .map(|(name, items)| {
                let state = state.clone();
                let open_with = open_with.clone();
                let builder: MenuBuilder = if name == "Tools" {
                    let commander = commander.clone();
                    Rc::new(move |menu, window, cx| {
                        let popup = cx.entity().downgrade();
                        let _ = commander.update(cx, |this, cx| {
                            this.discover_tools_for_menu(popup, true, window, cx)
                        });
                        tool_menu_footer(menu.label("Loading tools…"), &state)
                    })
                } else {
                    Rc::new(move |menu, window, cx| {
                        command_menu(menu, &items, &state, window, cx, open_with.clone())
                    })
                };
                (SharedString::from(name), builder)
            })
            .collect();
        self.menu_state = Some(state);
        self.menu_bar.update(cx, |bar, cx| bar.reload(menus, cx));
    }

    pub fn file_context_menu(
        mut menu: PopupMenu,
        window: &mut Window,
        cx: &mut App,
        state: menus::State,
        open_with: Option<(std::path::PathBuf, Vec<crate::apps::Choice>)>,
        commander: WeakEntity<Self>,
    ) -> PopupMenu {
        for item in menus::context_menu() {
            menu = match item {
                menus::Item::Cmd(Cmd::ToolsMenu) => {
                    let submenu = PopupMenu::build(window, cx, |submenu, _, _| {
                        submenu.label("Loading tools…")
                    });
                    let popup = submenu.downgrade();
                    let _ = commander.update(cx, |this, cx| {
                        this.discover_tools_for_menu(popup, false, window, cx)
                    });
                    menu.item(PopupMenuItem::submenu("Tools for Selection", submenu))
                }
                menus::Item::Cmd(cmd) => menu.menu_with_disabled(
                    cmd.label(),
                    action_for(cmd),
                    cmd.unavailable(&state).is_some(),
                ),
                menus::Item::OpenWith => match &open_with {
                    Some((path, choices)) => crate::commander::open_with::add_choice_submenu(
                        menu,
                        choices.clone(),
                        path.clone(),
                        commander.clone(),
                        window,
                        cx,
                    ),
                    None => menu.menu_with_disabled("Open With…", action_for(Cmd::OpenWith), true),
                },
                menus::Item::Separator => menu.separator(),
                menus::Item::Submenu(label, commands) => {
                    let submenu = PopupMenu::build(window, cx, |submenu, _, _| {
                        commands.iter().fold(submenu, |submenu, cmd| {
                            submenu.item(command_item(*cmd, &state))
                        })
                    });
                    menu.item(PopupMenuItem::submenu(label, submenu))
                }
            };
        }
        menu
    }

    /// F1: every command with its keys (the palette, as help).
    pub fn help(&mut self, _: &Help, window: &mut Window, cx: &mut Context<Self>) {
        self.open_palette("Help: commands and keys", window, cx);
    }

    /// Ctrl-P: a filterable list of all commands with their keys.
    pub fn command_palette(
        &mut self,
        _: &CommandPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_palette("Commands", window, cx);
    }

    pub fn open_palette(
        &mut self,
        title: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Keys are looked up from the panel, where all bindings apply.
        let panel = self.views[self.active].read(cx).focus_handle(cx);
        let commands = Self::palette_commands()
            .into_iter()
            .map(|(label, action)| {
                let keys = window
                    .bindings_for_action_in(action.as_ref(), &panel)
                    .iter()
                    .map(|binding| {
                        binding
                            .keystrokes()
                            .iter()
                            .map(|k| k.to_string())
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                Command {
                    label: label.to_string(),
                    keys,
                    payload: action,
                }
            })
            .collect();
        let delegate = PaletteDelegate::new(Palette::new(commands), move |action, window, cx| {
            window.close_dialog(cx);
            // Run once focus is back on the panel, so panel commands apply.
            window.defer(cx, move |window, cx| window.dispatch_action(action, cx));
        });
        let list = cx.new(|cx| ListState::new(delegate, window, cx).searchable(true));
        list.update(cx, |list, cx| {
            list.set_selected_index(Some(IndexPath::new(0)), window, cx)
        });
        let dialog_list = list.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            dialog.title(title).w(px(520.)).child(
                div()
                    .h(px(400.))
                    .child(List::new(&dialog_list).search_placeholder("Type a command…")),
            )
        });
        list.update(cx, |list, cx| list.focus(window, cx));
    }
}
