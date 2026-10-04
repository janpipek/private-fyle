//! List view over a [`ThemePicker`]: moving the selection previews the theme
//! live, confirming keeps it, cancelling restores the original.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::list::{ListDelegate, ListItem, ListState};
use gpui_kit::component::{ActiveTheme as _, IndexPath, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::themes::{self, ThemeChoice, ThemePicker};

type OnConfirm = Rc<dyn Fn(String, &mut Window, &mut App)>;

pub struct ThemePickerDelegate {
    picker: ThemePicker,
    selected: Option<usize>,
    /// Shared with the dialog so closing it after a confirm does not revert.
    confirmed: Rc<Cell<bool>>,
    on_confirm: OnConfirm,
}

impl ThemePickerDelegate {
    pub fn new(
        picker: ThemePicker,
        confirmed: Rc<Cell<bool>>,
        on_confirm: impl Fn(String, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            selected: picker.original_index(),
            picker,
            confirmed,
            on_confirm: Rc::new(on_confirm),
        }
    }

    pub fn original_index(&self) -> Option<usize> {
        self.picker.original_index()
    }
}

/// Re-applies `original` (used on cancel).
pub fn revert(original: &str, window: &mut Window, cx: &mut App) {
    let _ = themes::apply(&ThemeChoice::parse(original), window, cx);
}

impl ListDelegate for ThemePickerDelegate {
    type Item = ListItem;

    fn perform_search(
        &mut self,
        query: &str,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        self.picker.filter(query);
        self.selected = None;
        Task::ready(())
    }

    fn items_count(&self, _: usize, _: &App) -> usize {
        self.picker.len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let name = self.picker.get(ix.row)?.to_string();
        let is_original = self.picker.original_index() == Some(ix.row);
        Some(
            ListItem::new(ix.row)
                .selected(self.selected == Some(ix.row))
                .child(
                    h_flex()
                        .w_full()
                        .justify_between()
                        .child(name)
                        .when(is_original, |row| {
                            row.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("current"),
                            )
                        }),
                ),
        )
    }

    /// Selection moved: preview that theme.
    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        self.selected = ix.map(|ix| ix.row);
        if let Some(name) = self.selected.and_then(|row| self.picker.get(row)) {
            let _ = themes::apply(&ThemeChoice::parse(name), window, cx);
        }
    }

    fn confirm(&mut self, _: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        let Some(name) = self.selected.and_then(|row| self.picker.get(row)) else {
            return;
        };
        self.confirmed.set(true);
        (self.on_confirm)(name.to_string(), window, cx);
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        revert(self.picker.original(), window, cx);
    }
}
