//! Dialog for text and categorical rules on one panel column.

use std::collections::BTreeSet;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::column_filter::{CategoryValue, DraftMode, FilterDraft, MatchMode};
use crate::columns::{ColumnId, ColumnKind, ExifField};
use crate::model::{Folder, PanelContent};

/// A live editor that follows facet progress while the panel scan runs.
pub struct ColumnFilterEditor {
    folder: Entity<Folder>,
    source: PanelContent,
    column: ColumnId,
    label: String,
    draft: FilterDraft,
    input: Entity<InputState>,
    _input_subscription: Subscription,
    _folder_subscription: Subscription,
}

impl ColumnFilterEditor {
    /// Creates the editor and observes scan batches so category choices appear as acquired.
    pub fn new(
        folder: Entity<Folder>,
        source: PanelContent,
        column: ColumnId,
        label: String,
        draft: FilterDraft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Search values or enter text…")
                .default_value(draft.query.clone())
        });
        let _input_subscription = cx.observe(&input, |_, _, cx| cx.notify());
        let _folder_subscription = cx.observe(&folder, |_, _, cx| cx.notify());
        Self {
            folder,
            source,
            column,
            label,
            draft,
            input,
            _input_subscription,
            _folder_subscription,
        }
    }

    fn set_mode(&mut self, mode: DraftMode) {
        self.draft.mode = mode;
    }

    fn toggle(&mut self, value: CategoryValue, selected: bool) {
        if selected {
            self.draft.selected.insert(value);
        } else {
            self.draft.selected.remove(&value);
        }
    }
}

impl Render for ColumnFilterEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let folder = self.folder.read(cx);
        let source_stale = folder.content() != &self.source;
        let filter = folder.column_filter();
        let facets = filter.facets(&self.column);
        let diagnostics = filter
            .facet_diagnostics(&self.column)
            .cloned()
            .unwrap_or_default();
        let progress = filter.progress();
        let choices = facets
            .map(|facets| facets.values.clone())
            .unwrap_or_default();
        let overflow = facets.map_or(0, |facets| facets.overflow);
        let selected = self.draft.selected.clone();
        let query = self.input.read(cx).value().to_lowercase();
        let weak = cx.weak_entity();
        let choices_ui = choices
            .into_iter()
            .filter(|(value, _)| {
                selected.contains(value)
                    || query.is_empty()
                    || value.label().to_lowercase().contains(&query)
            })
            .map(|(value, count)| {
                let checked = selected.contains(&value);
                let text = if value == CategoryValue::Blank
                    && self.column.builtin() == Some(ColumnKind::GitStatus)
                {
                    "No reported change (blank)".to_owned()
                } else {
                    value.label().to_owned()
                };
                let label = format!("{text} ({count})");
                let id = SharedString::from(format!("facet-{}-{}", self.column.as_str(), label));
                let changed = value.clone();
                let weak = weak.clone();
                Checkbox::new(id)
                    .label(label)
                    .checked(checked)
                    .on_click(move |enabled, _, cx| {
                        let changed = changed.clone();
                        let _ = weak.update(cx, |this, cx| {
                            this.toggle(changed, *enabled);
                            cx.notify();
                        });
                    })
            });

        let selected = self.draft.selected.clone();
        let mode = self.draft.mode;
        let source = self.source.clone();
        let column = self.column.clone();
        let folder_to_apply = self.folder.clone();
        let input = self.input.clone();
        let apply = Button::new("apply-column-filter")
            .label("Apply")
            .primary()
            .small()
            .on_click(move |_, window, cx| {
                folder_to_apply.update(cx, |folder, cx| {
                    if folder.content() == &source {
                        let mut rules = folder.column_filter().rules();
                        rules.retain(|rule| rule.column != column);
                        let draft = FilterDraft {
                            mode,
                            query: input.read(cx).value().to_string(),
                            selected: selected.clone(),
                        };
                        if let Some(rule) = draft.rule(column.clone()) {
                            rules.push(rule);
                        }
                        folder.set_column_filters(rules);
                    }
                    cx.notify();
                });
                window.close_dialog(cx);
            });
        let apply = apply.disabled(source_stale);
        let folder_to_clear = self.folder.clone();
        let source = self.source.clone();
        let column = self.column.clone();
        let clear = Button::new("clear-column-filter")
            .label("Clear")
            .small()
            .on_click(move |_, window, cx| {
                folder_to_clear.update(cx, |folder, cx| {
                    if folder.content() == &source {
                        folder.clear_column_filter(&column);
                    }
                    cx.notify();
                });
                window.close_dialog(cx);
            });
        let clear = clear.disabled(source_stale);

        let category_enabled = category_enabled(self.column.builtin());
        let has_values = facets.is_some();
        let empty_complete =
            has_values && choices_is_empty(facets) && progress.completed >= progress.total;
        let status = if progress.total > 0 && progress.completed < progress.total {
            format!(
                "Checking {} of {} listed entries; {} pending, {} failed, {} unavailable",
                progress.completed,
                progress.total,
                progress.pending,
                progress.failed,
                progress.unavailable
            )
        } else {
            format!(
                "{} failed, {} unavailable",
                progress.failed, progress.unavailable
            )
        };
        let theme = cx.theme();
        let mut body = v_flex()
            .gap_2()
            .child(div().text_sm().child(format!("Filter {}", self.label)))
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        Button::new("filter-mode-text")
                            .label("Text")
                            .small()
                            .when(self.draft.mode == DraftMode::Text, |b| b.primary())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.set_mode(DraftMode::Text);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("filter-mode-values")
                            .label("Values")
                            .small()
                            .when(self.draft.mode == DraftMode::Values, |b| b.primary())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.set_mode(DraftMode::Values);
                                cx.notify();
                            })),
                    ),
            )
            .child(Input::new(&self.input).small());
        if category_enabled && self.draft.mode == DraftMode::Values {
            body = body.child(
                v_flex()
                    .max_h(px(240.))
                    .overflow_y_scrollbar()
                    .gap_1()
                    .children(choices_ui)
                    .when(!has_values || empty_complete, |list| {
                        list.child(div().text_xs().text_color(theme.muted_foreground).child(
                            if empty_complete {
                                "No readable values; use Text mode."
                            } else {
                                "Collecting category values…"
                            },
                        ))
                    })
                    .when(
                        diagnostics.failed > 0 || diagnostics.unavailable > 0,
                        |list| {
                            list.child(div().text_xs().text_color(theme.muted_foreground).child(
                                format!(
                                        "{} failed, {} unavailable{}",
                                        diagnostics.failed,
                                        diagnostics.unavailable,
                                        diagnostics
                                            .reason
                                            .as_ref()
                                            .map(|reason| format!("; {reason}"))
                                            .unwrap_or_default()
                                    ),
                            ))
                        },
                    ),
            );
            if overflow > 0 {
                body = body.child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child("More values omitted; narrow with the search above."),
                );
            }
        }
        if self.draft.mode == DraftMode::Values {
            body = body.child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("Match any selected value; match every column filter."),
            );
        } else {
            body = body.child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("Matches a case-insensitive substring of the displayed value."),
            );
        }
        body = body.child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(status),
        );
        if source_stale {
            body = body.child(
                div()
                    .text_xs()
                    .text_color(theme.danger)
                    .child("Folder changed; close and reopen this editor for the current folder."),
            );
        }
        v_flex()
            .gap_3()
            .p_2()
            .child(body)
            .child(h_flex().justify_end().gap_2().child(clear).child(apply))
    }
}

fn category_enabled(kind: Option<ColumnKind>) -> bool {
    matches!(
        kind,
        Some(
            ColumnKind::Ext
                | ColumnKind::GitStatus
                | ColumnKind::Owner
                | ColumnKind::Permissions
                | ColumnKind::Selinux
                | ColumnKind::Exif(ExifField::Camera | ExifField::Lens)
        )
    )
}

fn choices_is_empty(facets: Option<&crate::column_filter::FacetValues>) -> bool {
    facets.is_some_and(|facets| facets.values.is_empty())
}

/// Creates a pure editor draft from the existing rule or a column's default mode.
pub fn initial_rule(
    rules: &[crate::column_filter::ColumnRule],
    column: &ColumnId,
    category_default: bool,
) -> FilterDraft {
    match rules
        .iter()
        .find(|rule| &rule.column == column)
        .map(|rule| &rule.mode)
    {
        Some(MatchMode::Contains(value)) => FilterDraft {
            mode: DraftMode::Text,
            query: value.clone(),
            selected: BTreeSet::new(),
        },
        Some(MatchMode::AnyOf(values)) => FilterDraft {
            mode: DraftMode::Values,
            query: String::new(),
            selected: values.clone(),
        },
        None => FilterDraft {
            mode: if category_default {
                DraftMode::Values
            } else {
                DraftMode::Text
            },
            ..FilterDraft::default()
        },
    }
}
