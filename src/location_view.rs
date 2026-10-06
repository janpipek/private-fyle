//! Locations as a popup menu (long groups in submenus) and as a grouped,
//! searchable list; both only render [`crate::places`] data and forward the
//! chosen path.

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::list::{ListDelegate, ListItem, ListState};
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme as _, IndexPath, h_flex};
use gpui_kit::*;

use crate::palette::words_match;
use crate::places::{Group, Place};

type OnConfirm = Rc<dyn Fn(PathBuf, &mut Window, &mut App)>;

/// Builds the locations menu: short groups inline under a heading, long ones
/// (see [`Group::as_submenu`]) as scrollable submenus listing full paths, and
/// a last item that opens the searchable list.
pub fn locations_menu(
    groups: Vec<Group>,
    on_pick: impl Fn(PathBuf, &mut Window, &mut App) + 'static,
    on_search: impl Fn(&mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) -> Entity<PopupMenu> {
    let on_pick: OnConfirm = Rc::new(on_pick);
    let item = |label: String, path: PathBuf, on_pick: &OnConfirm| {
        let on_pick = on_pick.clone();
        PopupMenuItem::new(label).on_click(move |_, window, cx| on_pick(path.clone(), window, cx))
    };
    PopupMenu::build(window, cx, move |mut menu, window, cx| {
        for group in &groups {
            if group.as_submenu() {
                let places = group.places.clone();
                let on_pick = on_pick.clone();
                menu = menu.submenu(group.label.clone(), window, cx, move |menu, _, _| {
                    places
                        .iter()
                        .fold(menu.max_h(px(480.)).scrollable(true), |menu, place| {
                            let label = crate::places::tilde(&place.path);
                            menu.item(item(label, place.path.clone(), &on_pick))
                        })
                });
            } else {
                menu = menu.label(group.label.clone());
                for place in &group.places {
                    menu = menu.item(item(place.label.clone(), place.path.clone(), &on_pick));
                }
            }
        }
        menu.separator().item(
            PopupMenuItem::new("Search Locations… (Ctrl-G)")
                .on_click(move |_, window, cx| on_search(window, cx)),
        )
    })
}

/// List delegate for grouped location data.
pub struct LocationDelegate {
    groups: Vec<Group>,
    shown: Vec<Group>,
    selected: Option<IndexPath>,
    on_confirm: OnConfirm,
}

impl LocationDelegate {
    /// Creates a picker that forwards the confirmed path.
    pub fn new(
        groups: Vec<Group>,
        on_confirm: impl Fn(PathBuf, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            shown: groups.clone(),
            groups,
            selected: Some(IndexPath::new(0)),
            on_confirm: Rc::new(on_confirm),
        }
    }

    fn place(&self, ix: IndexPath) -> Option<&Place> {
        self.shown.get(ix.section)?.places.get(ix.row)
    }
}

impl ListDelegate for LocationDelegate {
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
            .filter_map(|group| {
                let places = group
                    .places
                    .iter()
                    .filter(|place| {
                        words_match(
                            &format!("{} {} {}", group.label, place.label, place.path.display()),
                            query,
                        )
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                (!places.is_empty()).then(|| Group {
                    label: group.label.clone(),
                    places,
                })
            })
            .collect();
        self.selected = (!self.shown.is_empty()).then(|| IndexPath::new(0));
        Task::ready(())
    }

    fn sections_count(&self, _: &App) -> usize {
        self.shown.len().max(1)
    }

    fn items_count(&self, section: usize, _: &App) -> usize {
        self.shown.get(section).map_or(0, |g| g.places.len())
    }

    fn render_section_header(
        &mut self,
        section: usize,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<impl IntoElement> {
        Some(
            div()
                .px_2()
                .pt_2()
                .pb_1()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(self.shown.get(section)?.label.clone()),
        )
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let place = self.place(ix)?.clone();
        Some(
            ListItem::new(ix.section * 1000 + ix.row)
                .selected(self.selected == Some(ix))
                .child(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .gap_3()
                        .child(div().flex_none().child(place.label))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(crate::places::tilde(&place.path)),
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
        self.selected = ix;
    }

    fn confirm(&mut self, _: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        if let Some(path) = self
            .selected
            .and_then(|ix| self.place(ix))
            .map(|place| place.path.clone())
        {
            (self.on_confirm)(path, window, cx);
        }
    }
}
