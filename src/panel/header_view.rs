//! Panel header rendering and event forwarding; input and focus lifecycle stay in FolderView.

use std::path::Path;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::location::PanelTarget;

use super::{FolderView, HeaderEvent, LOCATION_CONTEXT};

impl FolderView {
    /// Renders breadcrumbs, location editing and header buttons for this panel.
    pub fn render_header(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let focus = self.focus_handle(cx);
        // Tooltip action contexts accept a single KeyContext, not predicates
        // such as "Commander > DataTable" (GPUI 0.7 recurses forever on '>').
        // Resolve against this panel so inactive panels and icon view work too.
        let header_tooltip = |text: String, action: &dyn Action| match window
            .highest_precedence_binding_for_action_in(action, &focus)
        {
            Some(binding) => {
                let keys = binding
                    .keystrokes()
                    .iter()
                    .map(|key| Kbd::format(key.as_keystroke()))
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("{text} ({keys})")
            }
            None => text,
        };
        let theme = cx.theme();
        let star_color = theme.warning;
        let header_foreground = if self.active {
            theme.primary_foreground
        } else {
            theme.foreground
        };
        let folder = self.folder.read(cx);
        let path_text = crate::breadcrumbs::editable_path(&folder.path());
        let header_text = folder.header();
        let segments = crate::breadcrumbs::segments(
            &folder.path(),
            folder.archive(),
            folder.inner_path().as_deref(),
            crate::remote_labels::label_of(&folder.path(), cx).as_deref(),
        );
        let archive_source = folder.archive().map(Path::to_path_buf);
        let view = cx.weak_entity();
        let mut breadcrumb_items = Vec::new();
        let segment_count = segments.len();
        for (index, segment) in segments.into_iter().enumerate() {
            let path = segment.path;
            let archive_source = archive_source.clone();
            let label = match segment.kind {
                crate::breadcrumbs::Kind::Archive => format!("{} [archive]", segment.label),
                crate::breadcrumbs::Kind::ArchiveInner => format!("↳ {}", segment.label),
                crate::breadcrumbs::Kind::Normal | crate::breadcrumbs::Kind::Remote => {
                    segment.label
                }
            };
            let view = view.clone();
            breadcrumb_items.push(
                Button::new(("breadcrumb", index))
                    .label(label)
                    .ghost()
                    .compact()
                    .small()
                    .flex_none()
                    .text_color(header_foreground)
                    .accessibility_label(format!("Open {}", path.display()))
                    .on_click(move |_, _, cx| {
                        if let Some(view) = view.upgrade()
                            && let Ok(target) =
                                PanelTarget::from_panel_path(&path, archive_source.as_deref())
                        {
                            view.update(cx, |_, cx| cx.emit(HeaderEvent::Navigate(target)));
                        }
                    })
                    .into_any_element(),
            );
            if index + 1 < segment_count {
                breadcrumb_items.push(
                    Icon::new(IconName::ChevronRight)
                        .size_3()
                        .text_color(header_foreground)
                        .into_any_element(),
                );
            }
        }
        let locations_view = view.clone();
        let bookmark_view = view.clone();
        let edit_view = view.clone();
        let copy_view = view;
        self.breadcrumb_scroll
            .scroll_to_item(breadcrumb_items.len().saturating_sub(1));
        let breadcrumb_scroll = self.breadcrumb_scroll.clone();

        h_flex()
            .gap_2()
            .px_2()
            .py_1()
            .rounded(theme.radius)
            .when(self.active, |d| {
                d.bg(theme.primary).text_color(theme.primary_foreground)
            })
            .when(!self.active, |d| d.bg(theme.muted))
            .child(
                div()
                    .relative()
                    .flex_none()
                    .child(
                        Button::new("panel-locations")
                            .icon(Lucide::Map)
                            .ghost()
                            .compact()
                            .small()
                            .flex_none()
                            .text_color(header_foreground)
                            .tooltip(header_tooltip(
                                "Locations for this panel".into(),
                                &crate::commander::GoTo,
                            ))
                            .accessibility_label("Open locations for this panel")
                            .on_click(move |_, _, cx| {
                                if let Some(view) = locations_view.upgrade() {
                                    view.update(cx, |_, cx| cx.emit(HeaderEvent::Locations));
                                }
                            }),
                    )
                    .when_some(self.locations_menu.clone(), |d, menu| {
                        d.child(deferred(
                            anchored()
                                .anchor(Anchor::TopLeft)
                                .snap_to_window_with_margin(px(8.))
                                .child(div().occlude().top_1().child(menu)),
                        ))
                    }),
            )
            .child(
                div()
                    .id("breadcrumb-path")
                    .flex_1()
                    .min_w_0()
                    .overflow_x_scroll()
                    .track_scroll(&breadcrumb_scroll)
                    .when_some(self.location_input.clone(), |d, input| {
                        d.key_context(LOCATION_CONTEXT)
                            .on_action(cx.listener(Self::close_location))
                            .child(Input::new(&input).small())
                    })
                    .when(self.location_input.is_none(), |d| {
                        d.flex()
                            .flex_row()
                            .items_center()
                            .gap_0p5()
                            .children(breadcrumb_items)
                    }),
            )
            .when_some(self.bookmark, |d, on| {
                d.child(
                    Button::new("panel-bookmark")
                        .icon(Lucide::Star)
                        .ghost()
                        .compact()
                        .small()
                        .flex_none()
                        .text_color(if on { star_color } else { header_foreground })
                        .tooltip(header_tooltip(
                            if on {
                                "Remove this folder's bookmark".into()
                            } else {
                                "Bookmark this folder".into()
                            },
                            &crate::commander::ToggleBookmark,
                        ))
                        .accessibility_label("Toggle bookmark")
                        .on_click(move |_, _, cx| {
                            if let Some(view) = bookmark_view.upgrade() {
                                view.update(cx, |_, cx| cx.emit(HeaderEvent::ToggleBookmark));
                            }
                        }),
                )
            })
            .child(
                Button::new("panel-edit-location")
                    .icon(Lucide::SquareTerminal)
                    .ghost()
                    .compact()
                    .small()
                    .flex_none()
                    .text_color(header_foreground)
                    .tooltip(header_tooltip(
                        "Type or paste a location".into(),
                        &crate::commander::EditLocation,
                    ))
                    .accessibility_label("Edit full path")
                    .on_click(move |_, _, cx| {
                        if let Some(view) = edit_view.upgrade() {
                            view.update(cx, |_, cx| cx.emit(HeaderEvent::Edit));
                        }
                    }),
            )
            .child(
                Button::new("panel-copy-location")
                    .icon(Lucide::Copy)
                    .ghost()
                    .compact()
                    .small()
                    .flex_none()
                    .text_color(header_foreground)
                    .tooltip(header_tooltip(
                        format!("Copy full path: {header_text}"),
                        &crate::commander::CopyLocation,
                    ))
                    .accessibility_label("Copy full path")
                    .on_click(move |_, _, cx| {
                        if let Some(view) = copy_view.upgrade() {
                            let text = path_text.clone();
                            view.update(cx, |_, cx| cx.emit(HeaderEvent::CopyPath(text)));
                        }
                    }),
            )
            .when(folder.results().is_some(), |d| {
                d.child(div().flex_none().text_xs().child("RESULTS"))
            })
            .when(folder.is_tree() && folder.results().is_none(), |d| {
                d.child(div().flex_none().text_xs().child("TREE"))
            })
            .into_any_element()
    }
}
