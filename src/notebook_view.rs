//! Lazy GPUI rendering for the read-only notebook preview model.

use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::component::text::TextView;
use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::preview::{Notebook, NotebookCell as Cell, NotebookOutput as Output};

/// Prepares decoded notebook pixels for GPUI in the background preview worker.
pub fn prepare_images(notebook: &Notebook) -> HashMap<(usize, usize), Arc<RenderImage>> {
    let mut images = HashMap::new();
    for (cell_index, cell) in notebook.cells.iter().enumerate() {
        let Cell::Code { outputs, .. } = cell else {
            continue;
        };
        for (output_index, output) in outputs.iter().enumerate() {
            let Output::Image {
                width,
                height,
                rgba,
            } = output
            else {
                continue;
            };
            let mut pixels = image::RgbaImage::from_raw(*width, *height, rgba.clone())
                .expect("notebook model validates RGBA dimensions");
            for pixel in pixels.pixels_mut() {
                pixel.0.swap(0, 2);
            }
            images.insert(
                (cell_index, output_index),
                Arc::new(RenderImage::new(vec![image::Frame::new(pixels)])),
            );
        }
    }
    images
}

/// Renders notebook cells through GPUI's variable-height virtual list.
pub fn render(
    notebook: Arc<Notebook>,
    list_state: ListState,
    images: Arc<HashMap<(usize, usize), Arc<RenderImage>>>,
    cx: &App,
) -> AnyElement {
    let truncated = notebook.truncated;
    let cells = notebook.clone();
    let images_for_rows = images.clone();
    let list = list(list_state, move |index, _window, cx| {
        render_cell(
            index,
            &cells.cells[index],
            cells.language.as_deref(),
            &images_for_rows,
            cx,
        )
        .into_any_element()
    })
    .flex_1()
    .min_h_0()
    .size_full();
    let theme = cx.theme();
    v_flex()
        .size_full()
        .child(div().id("notebook-cells").flex_1().min_h_0().child(list))
        .when(truncated, |view| {
            view.child(
                div()
                    .px_2()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("Some notebook text or outputs were truncated to fit preview limits."),
            )
        })
        .into_any_element()
}

fn render_cell(
    index: usize,
    cell: &Cell,
    language: Option<&str>,
    images: &HashMap<(usize, usize), Arc<RenderImage>>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let shell = |label: &str, body: AnyElement| {
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
                    .child(label.to_string()),
            )
            .child(div().p_2().child(body))
            .into_any_element()
    };
    match cell {
        Cell::Markdown(text) => shell(
            "Markdown",
            div()
                .child(TextView::markdown(
                    format!("notebook-markdown-{index}"),
                    text.clone(),
                ))
                .into_any_element(),
        ),
        Cell::Code { source, outputs } => {
            let source = fenced_code(source, language);
            let code = TextView::markdown(format!("notebook-code-{index}"), source);
            let rendered_outputs = outputs.iter().enumerate().map(|(output_index, output)| {
                render_output(index, output_index, output, images, cx)
            });
            shell(
                "Code",
                v_flex()
                    .gap_2()
                    .child(code)
                    .children(rendered_outputs)
                    .into_any_element(),
            )
        }
        Cell::Raw(text) => shell(
            "Raw",
            div()
                .font_family(theme.mono_font_family.clone())
                .text_xs()
                .whitespace_normal()
                .child(text.clone())
                .into_any_element(),
        ),
        Cell::Placeholder(message) => placeholder(message, cx),
    }
}

fn render_output(
    cell_index: usize,
    output_index: usize,
    output: &Output,
    images: &HashMap<(usize, usize), Arc<RenderImage>>,
    cx: &App,
) -> AnyElement {
    match output {
        Output::Text { label, text } => {
            let body = TextView::markdown(
                format!("notebook-output-{cell_index}-{output_index}"),
                fenced_code(text, None),
            );
            v_flex()
                .flex_none()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(label.clone()),
                )
                .child(body)
                .into_any_element()
        }
        Output::Image { .. } => images
            .get(&(cell_index, output_index))
            .map(|image| {
                div()
                    .flex_none()
                    .max_w_full()
                    .max_h(px(480.))
                    .child(
                        img(ImageSource::Render(image.clone()))
                            .max_w_full()
                            .max_h(px(480.))
                            .object_fit(ObjectFit::Contain),
                    )
                    .into_any_element()
            })
            .unwrap_or_else(|| placeholder("Notebook image was not prepared", cx)),
        Output::Placeholder(message) => placeholder(message, cx),
    }
}

fn placeholder(message: &str, cx: &App) -> AnyElement {
    div()
        .flex_none()
        .my_1()
        .px_2()
        .py_1()
        .rounded(cx.theme().radius)
        .bg(cx.theme().muted)
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(message.to_string())
        .into_any_element()
}

fn fenced_code(source: &str, language: Option<&str>) -> String {
    let fence_len = source
        .split('\n')
        .map(|line| longest_backticks(line) + 1)
        .max()
        .unwrap_or(3)
        .max(3);
    let fence = "`".repeat(fence_len);
    let language = language
        .filter(|language| {
            !language.is_empty()
                && language
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '+'))
        })
        .unwrap_or("");
    format!("{fence}{language}\n{source}\n{fence}")
}

fn longest_backticks(line: &str) -> usize {
    let mut longest = 0;
    let mut current = 0;
    for ch in line.chars() {
        if ch == '`' {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    longest
}

#[cfg(test)]
mod tests {
    use super::fenced_code;

    #[test]
    fn code_fence_contains_backticks_and_rejects_language_injection() {
        let source = "print('```')\n```\nplain";
        let rendered = fenced_code(source, Some("python\n```\nHTML"));
        assert!(rendered.starts_with("````\n"));
        assert!(rendered.contains(source));
        assert!(rendered.ends_with("\n````"));
    }
}
