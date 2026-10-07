//! The Open With list: choices under group headings, unavailable ones
//! greyed out with the reason.

use std::rc::Rc;

use gpui_kit::component::list::{ListDelegate, ListItem, ListState};
use gpui_kit::component::{ActiveTheme as _, IndexPath, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::apps::{Choice, Group};
use crate::palette::words_match;

type Confirm = Rc<dyn Fn(usize, &mut Window, &mut App)>;

/// Choices (ordered by group) filtered by typed words; confirming reports
/// the index into the original choices.
pub struct OpenWithDelegate {
    choices: Vec<Choice>,
    /// Per visible group: its heading and the indices of its choices.
    shown: Vec<(Group, Vec<usize>)>,
    selected: Option<IndexPath>,
    on_confirm: Confirm,
}

impl OpenWithDelegate {
    pub fn new(
        choices: Vec<Choice>,
        on_confirm: impl Fn(usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        let mut delegate = Self {
            shown: Vec::new(),
            choices,
            selected: None,
            on_confirm: Rc::new(on_confirm),
        };
        delegate.filter("");
        delegate
    }

    fn filter(&mut self, query: &str) {
        let mut shown: Vec<(Group, Vec<usize>)> = Vec::new();
        for (ix, choice) in self.choices.iter().enumerate() {
            if !words_match(&choice.label(), query) {
                continue;
            }
            match shown.last_mut() {
                Some((group, items)) if *group == choice.group() => items.push(ix),
                _ => shown.push((choice.group(), vec![ix])),
            }
        }
        self.shown = shown;
        self.selected = self.first_available();
    }

    fn first_available(&self) -> Option<IndexPath> {
        self.shown
            .iter()
            .enumerate()
            .find_map(|(section, (_, items))| {
                items
                    .iter()
                    .position(|&ix| self.choices[ix].unavailable().is_none())
                    .map(|row| IndexPath::new(row).section(section))
            })
    }

    fn choice_ix(&self, ix: IndexPath) -> Option<usize> {
        self.shown.get(ix.section)?.1.get(ix.row).copied()
    }
}

impl ListDelegate for OpenWithDelegate {
    type Item = ListItem;

    fn perform_search(
        &mut self,
        query: &str,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        self.filter(query);
        Task::ready(())
    }

    fn sections_count(&self, _: &App) -> usize {
        self.shown.len().max(1)
    }

    fn items_count(&self, section: usize, _: &App) -> usize {
        self.shown.get(section).map_or(0, |(_, items)| items.len())
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
                .child(group.title()),
        )
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let choice = self.choices.get(self.choice_ix(ix)?)?;
        let note = choice.unavailable().map(String::from);
        let muted = cx.theme().muted_foreground;
        Some(
            ListItem::new(ix.section * 1000 + ix.row)
                .selected(self.selected == Some(ix))
                .child(
                    h_flex()
                        .w_full()
                        .justify_between()
                        .gap_4()
                        .when(note.is_some(), |row| row.text_color(muted))
                        .child(div().min_w_0().truncate().child(choice.label()))
                        .children(note.map(|n| div().text_xs().child(n))),
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
        let Some(ix) = self
            .selected
            .and_then(|ix| self.choice_ix(ix))
            .filter(|&ix| self.choices[ix].unavailable().is_none())
        else {
            return;
        };
        (self.on_confirm)(ix, window, cx);
    }
}
