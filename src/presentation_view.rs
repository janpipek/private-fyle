//! Scrollable GPUI rendering for the PowerPoint outline preview.

use std::sync::Arc;

use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::preview::Presentation;

/// Renders slides in presentation order as a virtual vertical list.
pub fn render(presentation: Arc<Presentation>, list_state: ListState, cx: &App) -> AnyElement {
    let slides = presentation.clone();
    let list = list(list_state.clone(), move |index, _window, cx| {
        let slide = &slides.slides[index];
        render_slide(index, slide, cx).into_any_element()
    })
    .flex_1()
    .min_h_0()
    .size_full();
    let list = div()
        .relative()
        .size_full()
        .child(list)
        .vertical_scrollbar(&list_state);
    let theme = cx.theme();
    v_flex()
        .size_full()
        .child(
            div()
                .id("presentation-slides")
                .flex_1()
                .min_h_0()
                .child(list),
        )
        .when(presentation.truncated, |view| {
            view.child(
                div()
                    .px_2()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("Presentation text was truncated to fit preview limits."),
            )
        })
        .into_any_element()
}

fn render_slide(index: usize, slide: &crate::preview::Slide, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let content = slide
        .text
        .iter()
        .enumerate()
        .map(|(paragraph_index, paragraph)| {
            div()
                .id(format!("slide-{index}-text-{paragraph_index}"))
                .whitespace_normal()
                .child(paragraph.clone())
                .into_any_element()
        });
    let images = slide.images.iter().map(|image| {
        div()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(format!("[image: {image}]"))
            .into_any_element()
    });
    v_flex()
        .w_full()
        .flex_none()
        .m_2()
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.border)
        .child(
            div()
                .px_2()
                .py_1()
                .bg(theme.muted)
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(format!("Slide {}", index + 1)),
        )
        .child(v_flex().gap_2().p_2().children(content).children(images))
        .into_any_element()
}
