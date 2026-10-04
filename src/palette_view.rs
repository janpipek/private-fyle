//! List view over a [`Palette`] of actions; confirming runs the action.

use std::rc::Rc;

use gpui_kit::component::list::{ListDelegate, ListItem, ListState};
use gpui_kit::component::{ActiveTheme as _, IndexPath, h_flex};
use gpui_kit::*;

use crate::palette::Palette;

type OnConfirm = Rc<dyn Fn(Box<dyn Action>, &mut Window, &mut App)>;

pub struct PaletteDelegate {
    palette: Palette<Box<dyn Action>>,
    selected: Option<usize>,
    on_confirm: OnConfirm,
}

impl PaletteDelegate {
    pub fn new(
        palette: Palette<Box<dyn Action>>,
        on_confirm: impl Fn(Box<dyn Action>, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            palette,
            selected: Some(0),
            on_confirm: Rc::new(on_confirm),
        }
    }
}

impl ListDelegate for PaletteDelegate {
    type Item = ListItem;

    fn perform_search(
        &mut self,
        query: &str,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        self.palette.filter(query);
        self.selected = (self.palette.len() > 0).then_some(0);
        Task::ready(())
    }

    fn items_count(&self, _: usize, _: &App) -> usize {
        self.palette.len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let command = self.palette.get(ix.row)?;
        Some(
            ListItem::new(ix.row)
                .selected(self.selected == Some(ix.row))
                .child(
                    h_flex()
                        .w_full()
                        .justify_between()
                        .gap_4()
                        .child(command.label.clone())
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(command.keys.clone()),
                        ),
                ),
        )
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) {
        self.selected = ix.map(|ix| ix.row);
    }

    fn confirm(&mut self, _: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        let Some(command) = self.selected.and_then(|row| self.palette.get(row)) else {
            return;
        };
        (self.on_confirm)(command.payload.boxed_clone(), window, cx);
    }
}
