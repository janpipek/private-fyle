//! List views for tools: the grouped menu of offers and the pick list for
//! [`Action::Pick`](crate::tools::Action::Pick).

use std::rc::Rc;

use gpui_kit::component::list::{ListDelegate, ListItem, ListState};
use gpui_kit::component::{ActiveTheme as _, IndexPath, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::palette::words_match;
use crate::tools::Offer;

/// Offers grouped into sections, filtered by typed words.
pub struct MenuDelegate {
    groups: Vec<(String, Vec<Offer>)>,
    /// `groups` narrowed by the query.
    shown: Vec<(String, Vec<Offer>)>,
    selected: Option<IndexPath>,
    on_confirm: Rc<dyn Fn(Offer, &mut Window, &mut App)>,
}

impl MenuDelegate {
    /// `offers` must already be ordered by group.
    pub fn new(
        offers: Vec<Offer>,
        on_confirm: impl Fn(Offer, &mut Window, &mut App) + 'static,
    ) -> Self {
        let mut groups: Vec<(String, Vec<Offer>)> = Vec::new();
        for offer in offers {
            match groups.last_mut() {
                Some((group, items)) if *group == offer.group => items.push(offer),
                _ => groups.push((offer.group.clone(), vec![offer])),
            }
        }
        Self {
            shown: groups.clone(),
            groups,
            selected: Some(IndexPath::new(0)),
            on_confirm: Rc::new(on_confirm),
        }
    }

    fn offer(&self, ix: IndexPath) -> Option<&Offer> {
        self.shown.get(ix.section)?.1.get(ix.row)
    }
}

impl ListDelegate for MenuDelegate {
    type Item = ListItem;

    fn perform_search(
        &mut self,
        query: &str,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        self.shown = self
            .groups
            .iter()
            .map(|(group, offers)| {
                let offers = offers
                    .iter()
                    .filter(|o| words_match(&format!("{group} {}", o.label), query))
                    .cloned()
                    .collect::<Vec<_>>();
                (group.clone(), offers)
            })
            .filter(|(_, offers)| !offers.is_empty())
            .collect();
        self.selected = (!self.shown.is_empty()).then(|| IndexPath::new(0));
        Task::ready(())
    }

    fn sections_count(&self, _: &App) -> usize {
        self.shown.len().max(1)
    }

    fn items_count(&self, section: usize, _: &App) -> usize {
        self.shown
            .get(section)
            .map_or(0, |(_, offers)| offers.len())
    }

    fn render_section_header(
        &mut self,
        section: usize,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<impl IntoElement> {
        let (group, _) = self.shown.get(section)?;
        Some(
            div()
                .px_2()
                .pt_2()
                .pb_1()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(group.clone()),
        )
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let offer = self.offer(ix)?.clone();
        let disabled = !offer.missing.is_empty();
        let muted = cx.theme().muted_foreground;
        Some(
            ListItem::new(ix.section * 1000 + ix.row)
                .selected(self.selected == Some(ix))
                .child(
                    h_flex()
                        .w_full()
                        .justify_between()
                        .gap_4()
                        .when(disabled, |row| row.text_color(muted))
                        .child(offer.label.clone())
                        .when(disabled, |row| {
                            row.child(
                                div()
                                    .text_xs()
                                    .child(format!("needs {}", offer.missing.join(", "))),
                            )
                        }),
                ),
        )
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) {
        self.selected = ix;
    }

    fn confirm(&mut self, _: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        // Tools missing a program cannot be chosen.
        if let Some(offer) = self
            .selected
            .and_then(|ix| self.offer(ix))
            .filter(|o| o.missing.is_empty())
        {
            (self.on_confirm)(offer.clone(), window, cx);
        }
    }
}

/// A plain list of labels; confirming reports the chosen index.
pub struct PickDelegate {
    labels: Vec<String>,
    /// Indices into `labels` matching the query.
    shown: Vec<usize>,
    selected: Option<usize>,
    on_confirm: Rc<dyn Fn(usize, &mut Window, &mut App)>,
}

impl PickDelegate {
    pub fn new(
        labels: Vec<String>,
        on_confirm: impl Fn(usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            shown: (0..labels.len()).collect(),
            labels,
            selected: Some(0),
            on_confirm: Rc::new(on_confirm),
        }
    }
}

impl ListDelegate for PickDelegate {
    type Item = ListItem;

    fn perform_search(
        &mut self,
        query: &str,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        self.shown = (0..self.labels.len())
            .filter(|&i| words_match(&self.labels[i], query))
            .collect();
        self.selected = (!self.shown.is_empty()).then_some(0);
        Task::ready(())
    }

    fn items_count(&self, _: usize, _: &App) -> usize {
        self.shown.len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let label = self.labels.get(*self.shown.get(ix.row)?)?.clone();
        Some(
            ListItem::new(ix.row)
                .selected(self.selected == Some(ix.row))
                .child(div().w_full().min_w_0().truncate().child(label)),
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
        if let Some(&index) = self.selected.and_then(|row| self.shown.get(row)) {
            (self.on_confirm)(index, window, cx);
        }
    }
}
