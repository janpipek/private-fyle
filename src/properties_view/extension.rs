//! Native selectable rendering for one extension Properties section.

use gpui_kit::base::SelectableText;
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::*;

use crate::data_providers::ResultState;
use crate::extensions::properties::PreparedSection;
use crate::extensions::results::PropertiesResult;

/// Renders one visible extension section using the caller's shared text ordering.
pub fn render(
    section: &PreparedSection,
    cx: &App,
    text: &mut impl FnMut(&str) -> SelectableText,
) -> Option<AnyElement> {
    if !section.is_visible() {
        return None;
    }
    let theme = &cx.theme().colors;
    let mut content = v_flex().w_full().gap_1().child(
        div()
            .font_weight(FontWeight::SEMIBOLD)
            .child(text(&section.label)),
    );
    match &section.result {
        PropertiesResult::Ready(fields) => {
            for field in fields {
                let label = humanize(field.field.as_str());
                match &field.state {
                    ResultState::Ready {
                        value,
                        completeness,
                    } => {
                        let display = crate::columns::cell::project(&ResultState::Ready {
                            value: value.clone(),
                            completeness: completeness.clone(),
                        });
                        let value = display.tooltip.map_or(display.text.clone(), |tooltip| {
                            format!("{} — {tooltip}", display.text)
                        });
                        content = content.child(field_row(
                            &label,
                            &value,
                            theme.muted_foreground,
                            theme.foreground,
                            text,
                        ));
                    }
                    ResultState::Failed(reason) => {
                        content = content.child(field_row(
                            &label,
                            &format!("Unavailable: {reason}"),
                            theme.muted_foreground,
                            theme.muted_foreground,
                            text,
                        ));
                    }
                    ResultState::Pending | ResultState::Absent | ResultState::Unavailable(_) => {}
                }
            }
        }
        PropertiesResult::Failed(reason) => {
            content = content.child(
                div()
                    .text_color(theme.muted_foreground)
                    .child(text(&format!("Failed: {reason}"))),
            );
        }
        PropertiesResult::Absent | PropertiesResult::Unavailable(_) => return None,
    }
    Some(content.into_any_element())
}

fn field_row(
    label: &str,
    value: &str,
    label_color: gpui_kit::Hsla,
    value_color: gpui_kit::Hsla,
    text: &mut impl FnMut(&str) -> SelectableText,
) -> AnyElement {
    h_flex()
        .w_full()
        .gap_2()
        .child(
            div()
                .w(px(120.))
                .flex_none()
                .text_color(label_color)
                .child(text(label)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_sm()
                .text_color(value_color)
                .child(text(value)),
        )
        .into_any_element()
}

fn humanize(field_id: &str) -> String {
    let segment = field_id.rsplit('.').next().unwrap_or(field_id);
    let mut label = segment.replace(['_', '-'], " ");
    if let Some(first) = label.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    label
}
