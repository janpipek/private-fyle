//! Native, selectable rendering for validated extension preview scenes.

use gpui_kit::base::SelectableText;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::*;

use crate::data_providers::{Completeness, ResultState, Value};
use crate::preview_scene::{
    Axis, Column, Gap, Node, NodeKind, Path, PathRole, Scene, StrokeWeight, TextRole,
};

/// Renders a validated scene with native selectable text, tables, and paths.
pub fn render(scene: &Scene, cx: &App) -> AnyElement {
    let mut selection = SelectionOrder { next: 0 };
    div()
        .size_full()
        .min_w_0()
        .p_2()
        .overflow_y_scrollbar()
        .child(render_node(scene.root(), cx, &mut selection))
        .into_any_element()
}

struct SelectionOrder {
    next: u64,
}

impl SelectionOrder {
    fn text(&mut self, text: &str) -> SelectableText {
        let order = self.next;
        self.next = self.next.saturating_add(1);
        SelectableText::new(format!("preview-scene-text-{order}"), text.to_owned())
            .document_order(order)
    }
}

fn render_node(node: &Node, cx: &App, selection: &mut SelectionOrder) -> AnyElement {
    match node.kind() {
        NodeKind::Stack {
            axis,
            gap,
            children,
        } => {
            let children = children
                .iter()
                .map(|child| render_node(child, cx, selection));
            match axis {
                Axis::Row => {
                    let row = h_flex().w_full().min_w_0();
                    let row = match gap {
                        Gap::Small => row.gap_1(),
                        Gap::Normal => row.gap_2(),
                    };
                    children
                        .fold(row, |layout, child| layout.child(child))
                        .into_any_element()
                }
                Axis::Column => {
                    let column = v_flex().w_full().min_w_0();
                    let column = match gap {
                        Gap::Small => column.gap_1(),
                        Gap::Normal => column.gap_2(),
                    };
                    children
                        .fold(column, |layout, child| layout.child(child))
                        .into_any_element()
                }
            }
        }
        NodeKind::Text { text, role } => {
            let theme = &cx.theme().colors;
            let selected = selection.text(text);
            match role {
                TextRole::Body => div().min_w_0().text_color(theme.foreground).child(selected),
                TextRole::Heading => div()
                    .min_w_0()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.foreground)
                    .child(selected),
                TextRole::Muted => div()
                    .min_w_0()
                    .text_color(theme.muted_foreground)
                    .child(selected),
            }
            .into_any_element()
        }
        NodeKind::Table { columns, rows } => render_table(columns, rows, cx, selection),
        NodeKind::Canvas {
            width,
            height,
            paths,
        } => render_canvas(*width, *height, paths, cx),
    }
}

fn render_table(
    columns: &[Column],
    rows: &[Vec<Option<Value>>],
    cx: &App,
    selection: &mut SelectionOrder,
) -> AnyElement {
    let theme = &cx.theme().colors;
    let table_width = px((columns.len() * 144) as f32);
    let mut table = v_flex().w(table_width).flex_none().gap_1();
    let mut heading = h_flex().w(table_width).flex_none();
    for column in columns {
        heading = heading.child(
            div()
                .w(px(144.))
                .flex_none()
                .px_2()
                .py_1()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.foreground)
                .child(selection.text(column.label())),
        );
    }
    table = table.child(heading);
    for row in rows {
        let mut rendered_row = h_flex().w(table_width).flex_none();
        for cell in row {
            let text = cell.as_ref().map_or_else(String::new, format_value);
            rendered_row = rendered_row.child(
                div()
                    .w(px(144.))
                    .flex_none()
                    .px_2()
                    .py_1()
                    .text_color(theme.foreground)
                    .child(selection.text(&text)),
            );
        }
        table = table.child(rendered_row);
    }
    div()
        .w_full()
        .min_w_0()
        .overflow_x_scrollbar()
        .child(table)
        .into_any_element()
}

fn format_value(value: &Value) -> String {
    crate::columns::cell::project(&ResultState::Ready {
        value: value.clone(),
        completeness: Completeness::Complete,
    })
    .text
}

fn render_canvas(width: f64, height: f64, paths: &[Path], cx: &App) -> AnyElement {
    let theme = &cx.theme().colors;
    let paths = paths
        .iter()
        .map(|path| {
            let color = match path.role() {
                PathRole::Foreground => theme.foreground,
                PathRole::Muted => theme.muted_foreground,
                PathRole::Accent => theme.primary,
                PathRole::Success => theme.success,
                PathRole::Warning => theme.warning,
            };
            (
                path.weight(),
                path.points()
                    .iter()
                    .map(|point| (point.x(), point.y()))
                    .collect::<Vec<_>>(),
                color,
            )
        })
        .collect::<Vec<_>>();
    let element = canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            let scale = (bounds.size.width.as_f32() / width as f32)
                .min(bounds.size.height.as_f32() / height as f32);
            for (weight, points, color) in &paths {
                let stroke = match weight {
                    StrokeWeight::Thin => 1.0,
                    StrokeWeight::Normal => 2.0,
                    StrokeWeight::Strong => 3.0,
                };
                let mut builder = PathBuilder::stroke(px(stroke));
                if let Some((x, y)) = points.first() {
                    builder.move_to(point(
                        bounds.origin.x + px(*x as f32 * scale),
                        bounds.origin.y + px(*y as f32 * scale),
                    ));
                    for (x, y) in points.iter().skip(1) {
                        builder.line_to(point(
                            bounds.origin.x + px(*x as f32 * scale),
                            bounds.origin.y + px(*y as f32 * scale),
                        ));
                    }
                    if let Ok(path) = builder.build() {
                        window.paint_path(path, *color);
                    }
                }
            }
        },
    );
    div()
        .w_full()
        .min_w_0()
        .aspect_ratio((width / height) as f32)
        .overflow_hidden()
        .child(element.size_full())
        .into_any_element()
}
