//! Appearance: columns, themes, zoom, settings file and the About dialog.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::list::{List, ListState};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IndexPath, Theme, ThemeRegistry, WindowExt as _, v_flex,
};
use gpui_kit::*;

use super::Commander;
use super::keys::{About, OpenConfig, SelectColumns, SelectTheme, ZoomIn, ZoomOut, ZoomReset};
use crate::columns::ColumnKind;
use crate::editor::Document;
use crate::settings::{self};
use crate::theme_picker::{self, ThemePickerDelegate};
use crate::themes::ThemePicker;
use crate::zoom;

/// Zooms the UI by changing the theme's font sizes; returns the new size.
pub fn set_font_size(cx: &mut App, f: impl FnOnce(f32) -> f32) -> f32 {
    let size = f(f32::from(cx.theme().font_size));
    Theme::update(cx, |theme| {
        theme.font_size = px(size);
        theme.mono_font_size = px(zoom::mono(size));
    });
    size
}

impl Commander {
    /// Opens the theme picker; the highlighted theme is previewed live.
    /// Columns…: the column catalog, grouped, as checkboxes. Changes apply
    /// to both panels at once and are saved to the settings file.
    pub fn select_columns(
        &mut self,
        _: &SelectColumns,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let this = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            let Some(commander) = this.upgrade() else {
                return dialog;
            };
            let shown = commander.read(cx).views[0].read(cx).columns(cx);
            let muted = cx.theme().muted_foreground;
            let groups = crate::columns::catalog()
                .into_iter()
                .map(|(group, columns)| {
                    v_flex()
                        .gap_1()
                        .child(div().text_xs().text_color(muted).child(group.to_string()))
                        .children(columns.into_iter().map(|column| {
                            let this = this.clone();
                            let id = SharedString::from(format!("column-{}", column.id()));
                            Checkbox::new(id)
                                .label(column.label())
                                .checked(shown.contains(&column))
                                .disabled(column == ColumnKind::Name)
                                .on_click(move |checked, _, cx| {
                                    let _ = this.update(cx, |this, cx| {
                                        this.toggle_column(column, *checked, cx);
                                    });
                                })
                        }))
                });
            dialog.title("Columns").w(px(360.)).child(
                div()
                    .id("columns")
                    .max_h(px(480.))
                    .overflow_y_scroll()
                    .child(v_flex().gap_3().children(groups)),
            )
        });
    }

    pub fn toggle_column(&mut self, column: ColumnKind, show: bool, cx: &mut Context<Self>) {
        let current = self.views[0].read(cx).columns(cx);
        let columns = crate::columns::toggle(&current, column, show);
        for view in &self.views {
            view.update(cx, |view, cx| view.set_columns(columns.clone(), cx));
        }
        if let Err(e) = settings::persistence::save_columns(&columns) {
            self.status = e.into();
        }
        cx.notify();
    }

    pub fn select_theme(&mut self, _: &SelectTheme, window: &mut Window, cx: &mut Context<Self>) {
        let registry = ThemeRegistry::global(cx);
        let defaults = [
            registry.default_light_theme().name.clone(),
            registry.default_dark_theme().name.clone(),
        ];
        let names = registry
            .themes()
            .keys()
            .filter(|name| !defaults.contains(name)) // offered as "Light"/"Dark"
            .map(|name| name.to_string())
            .collect::<Vec<_>>();
        let picker = ThemePicker::new(names, &self.theme);

        let confirmed = Rc::new(Cell::new(false));
        let this = cx.entity().downgrade();
        let delegate =
            ThemePickerDelegate::new(picker, confirmed.clone(), move |name, window, cx| {
                let _ = this.update(cx, |this, cx| this.theme_chosen(name, cx));
                window.close_dialog(cx);
            });
        let original_ix = delegate.original_index();
        let list = cx.new(|cx| ListState::new(delegate, window, cx).searchable(true));
        list.update(cx, |list, cx| {
            if let Some(ix) = original_ix {
                list.set_selected_index(Some(IndexPath::new(ix)), window, cx);
                list.scroll_to_selected_item(window, cx);
            }
        });

        let original = self.theme.clone();
        let dialog_list = list.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let (original, confirmed) = (original.clone(), confirmed.clone());
            dialog
                .title("Theme")
                .w(px(420.))
                .child(
                    div()
                        .h(px(380.))
                        .child(List::new(&dialog_list).search_placeholder("Filter themes…")),
                )
                .on_close(move |_, window, cx| {
                    if !confirmed.get() {
                        theme_picker::revert(&original, window, cx);
                    }
                })
        });
        list.update(cx, |list, cx| list.focus(window, cx));
    }

    pub fn theme_chosen(&mut self, name: String, cx: &mut Context<Self>) {
        self.status = match settings::persistence::save_theme(&name) {
            Ok(path) => format!("Theme: {name} (saved to {})", path.display()).into(),
            Err(e) => format!("Theme: {name} (not saved: {e})").into(),
        };
        self.theme = name;
        cx.notify();
    }

    pub fn zoom_in(&mut self, _: &ZoomIn, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom(cx, |size| zoom::step(size, 1));
    }

    pub fn zoom_out(&mut self, _: &ZoomOut, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom(cx, |size| zoom::step(size, -1));
    }

    pub fn zoom_reset(&mut self, _: &ZoomReset, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom(cx, |_| zoom::DEFAULT);
    }

    /// Changes the font size and saves it to `config.toml` once zooming
    /// pauses, so a burst of Ctrl-wheel steps writes the file once.
    pub fn zoom(&mut self, cx: &mut Context<Self>, f: impl FnOnce(f32) -> f32) {
        let size = set_font_size(cx, f);
        self._zoom_save = cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(500))
                .await;
            if let Err(e) = settings::persistence::save_font_size(size) {
                let _ = this.update(cx, |this, cx| {
                    this.status = e.into();
                    cx.notify();
                });
            }
        });
    }

    /// Help › Configuration: edits `config.toml`, created from the example
    /// first if missing.
    pub fn open_config(&mut self, _: &OpenConfig, window: &mut Window, cx: &mut Context<Self>) {
        match settings::persistence::ensure_file() {
            Ok(path) => {
                self.open_in_editor(path.clone(), Document::open(&path), window, cx);
                if self.editor_command.is_empty() {
                    self.status = format!(
                        "Editing {}; most settings apply after a restart",
                        path.display()
                    )
                    .into();
                }
            }
            Err(e) => self.status = e.into(),
        }
        cx.notify();
    }

    /// Help › About.
    pub fn about(&mut self, _: &About, window: &mut Window, cx: &mut Context<Self>) {
        window.open_alert_dialog(cx, |dialog, _, cx| {
            let muted = cx.theme().muted_foreground;
            dialog.title(format!("Fyle {}", env!("CARGO_PKG_VERSION"))).description(
                v_flex()
                    .gap_2()
                    .child("A two-panel file manager in the spirit of Krusader and Midnight Commander.")
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child("Built with GPUI Kit. Licensed under the GPL, version 3 or later."),
                    ),
            )
        });
    }
}
