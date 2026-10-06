//! The right-click file menu, shared by the list and the icon grid: the
//! clicked entry becomes the target (keeping marks it belongs to), then the
//! commander's builder adds the commands.

use std::rc::Rc;

use gpui_kit::component::menu::PopupMenu;
use gpui_kit::*;

use crate::model::Folder;

/// Fills a popup menu with the file commands (set by the commander).
pub type ContextMenuBuilder = Rc<dyn Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu>;

/// Menu for a right-click on entry `row`: targets it in `folder`, then lets
/// `build` (if set) add the commands.
pub fn file_menu(
    folder: &Entity<Folder>,
    row: usize,
    build: Option<&ContextMenuBuilder>,
    menu: PopupMenu,
    window: &mut Window,
    cx: &mut App,
) -> PopupMenu {
    folder.update(cx, |folder, cx| {
        folder.select_context_row(row);
        cx.notify();
    });
    match build {
        Some(build) => build(menu, window, cx),
        None => menu,
    }
}
