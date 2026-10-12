//! View › List View / Icon View / Larger Icons / Smaller Icons: how the
//! active panel shows its folder, saved to `config.toml`.

use gpui_kit::*;

use super::Commander;
use super::keys::{IconView, LargerIcons, ListView, SmallerIcons};
use crate::grid::{self, ViewMode};
use crate::menus::Cmd;
use crate::settings;

impl Commander {
    pub fn list_view(&mut self, _: &ListView, _: &mut Window, cx: &mut Context<Self>) {
        self.set_view_mode(ViewMode::List, cx);
    }

    pub fn icon_view(&mut self, _: &IconView, _: &mut Window, cx: &mut Context<Self>) {
        self.set_view_mode(ViewMode::Icons, cx);
    }

    pub fn larger_icons(&mut self, _: &LargerIcons, _: &mut Window, cx: &mut Context<Self>) {
        if self.icon_size_unavailable(cx) {
            return;
        }
        self.step_icon_size(1, cx);
    }

    pub fn smaller_icons(&mut self, _: &SmallerIcons, _: &mut Window, cx: &mut Context<Self>) {
        if self.icon_size_unavailable(cx) {
            return;
        }
        self.step_icon_size(-1, cx);
    }

    /// The size keys also reach the list; there they only say why not.
    fn icon_size_unavailable(&mut self, cx: &mut Context<Self>) -> bool {
        if self.views[self.active].read(cx).view_mode() == ViewMode::Icons {
            return false;
        }
        self.status = Cmd::LargerIcons
            .unavailable(&self.menu_state(cx))
            .unwrap_or_default()
            .into();
        cx.notify();
        true
    }

    /// Switches the active panel's view. Tree mode does not apply to icons,
    /// so it is switched off; the cursor entry stays.
    pub fn set_view_mode(&mut self, mode: ViewMode, cx: &mut Context<Self>) {
        let ix = self.active;
        if self.views[ix].read(cx).view_mode() == mode {
            return;
        }
        if mode == ViewMode::Icons {
            let error = self.update_folder(cx, |folder| {
                if folder.is_tree() {
                    folder.set_tree(false)
                } else {
                    None
                }
            });
            self.set_error(error.map_or(Ok(()), Err), cx);
        }
        self.views[ix].update(cx, |view, cx| view.set_view_mode(mode, cx));
        self.save_setting(settings::persistence::save_panel_view(ix, mode), cx);
    }

    /// Both panels share the icon size, so the setting stays one number.
    fn step_icon_size(&mut self, steps: i32, cx: &mut Context<Self>) {
        let current = self.views[self.active].read(cx).icon_size(cx);
        let size = grid::step_size(current, steps);
        if size == current {
            return;
        }
        for view in &self.views {
            view.update(cx, |view, cx| view.set_icon_size(size, cx));
        }
        self.save_setting(settings::persistence::save_icon_size(size), cx);
    }

    fn save_setting(&mut self, saved: Result<std::path::PathBuf, String>, cx: &mut Context<Self>) {
        if let Err(e) = saved {
            self.status = e.into();
        }
        cx.notify();
    }
}
