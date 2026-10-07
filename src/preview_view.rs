//! Quick-view pane: shows a [`Preview`] of a path. Loading runs in the
//! background; each [`Content`] variant has one render arm here.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::component::input::{Editor, EditorState, Position};
use gpui_kit::component::text::TextView;
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::fs_ops;
use crate::preview::{Content, Preview, Registry};

enum State {
    Empty,
    Loading,
    Ready(Preview),
    Failed(String),
}

/// How long the cursor must rest on an entry before its preview loads.
const LOAD_DELAY: Duration = Duration::from_millis(60);

#[derive(Clone, Debug, PartialEq, Eq)]
struct PreviewTarget {
    path: Option<PathBuf>,
    identity: Option<crate::location::Location>,
}

pub struct PreviewPane {
    registry: Arc<Registry>,
    target: Option<PreviewTarget>,
    /// Line (1-based) to show in text content, e.g. a search hit's.
    line: Option<u64>,
    state: State,
    /// Reused read-only editor for text content.
    editor: Entity<EditorState>,
    _load: Task<()>,
    _reveal: Task<()>,
}

impl PreviewPane {
    pub fn new(registry: Arc<Registry>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .line_number(true)
                .soft_wrap(false)
                .folding(false)
        });
        Self {
            registry,
            target: None,
            line: None,
            state: State::Empty,
            editor,
            _load: Task::ready(()),
            _reveal: Task::ready(()),
        }
    }

    /// Shows local bytes while retaining the typed item they represent.
    pub fn set_location_target(
        &mut self,
        path: Option<PathBuf>,
        identity: Option<crate::location::Location>,
        line: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target =
            (path.is_some() || identity.is_some()).then_some(PreviewTarget { path, identity });
        if target == self.target {
            if line != self.line {
                self.line = line;
                self.reveal_line(window, cx);
            }
            return;
        }
        self.line = line;
        self.target = target.clone();
        let Some(path) = target.and_then(|target| target.path) else {
            self.state = State::Empty;
            self._load = Task::ready(());
            cx.notify();
            return;
        };
        self.state = State::Loading;
        let registry = self.registry.clone();
        self._load = cx.spawn_in(window, async move |this, cx| {
            // Holding an arrow key moves the cursor faster than this; the
            // next target drops the task, so only the entry it stops on loads.
            cx.background_executor().timer(LOAD_DELAY).await;
            let result = cx
                .background_spawn(async move { registry.preview(&path) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| this.show(result, window, cx));
        });
        cx.notify();
    }

    /// Loads the current target again (e.g. after it was edited).
    pub fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.target.take();
        self.set_location_target(
            target.as_ref().and_then(|target| target.path.clone()),
            target.and_then(|target| target.identity),
            self.line,
            window,
            cx,
        );
    }

    /// Moves the text view to [`Self::line`] without taking focus. Again
    /// after a moment too: scrolling needs the editor's layout, which new
    /// content only has once drawn.
    fn reveal_line(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(line) = self.line else {
            self._reveal = Task::ready(());
            return;
        };
        if !matches!(&self.state, State::Ready(p) if matches!(p.content, Content::Text { .. })) {
            return;
        }
        let editor = self.editor.clone();
        let reveal = move |window: &mut Window, cx: &mut App| {
            // Moving the cursor focuses the editor; give focus back.
            let focused = window.focused(cx);
            let position = Position::new(line.saturating_sub(1) as u32, 0);
            editor.update(cx, |editor, cx| {
                editor.set_cursor_position(position, window, cx)
            });
            if let Some(focused) = focused {
                focused.focus(window, cx);
            }
        };
        reveal(window, cx);
        self._reveal = cx.spawn_in(window, async move |_, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(50))
                .await;
            let _ = cx.update(|window, cx| reveal(window, cx));
        });
    }

    fn show(
        &mut self,
        result: Result<Preview, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state = match result {
            Ok(preview) => {
                if let Content::Text { text, language, .. } = &preview.content {
                    self.editor.update(cx, |editor, cx| {
                        editor.set_highlighter(language.unwrap_or("text"), cx);
                        editor.set_value(text.clone(), window, cx);
                    });
                }
                State::Ready(preview)
            }
            Err(e) => State::Failed(e),
        };
        self.reveal_line(window, cx);
        cx.notify();
    }

    fn render_content(&self, preview: &Preview, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let note = |text: String| {
            div()
                .px_2()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(text)
        };
        match &preview.content {
            Content::Text { truncated, .. } => v_flex()
                .size_full()
                .child(
                    div().flex_1().min_h_0().child(
                        Editor::new(&self.editor)
                            .readonly(true)
                            .bordered(false)
                            .h_full(),
                    ),
                )
                .when(*truncated, |d| {
                    d.child(note("Showing the first 512 KiB.".into()))
                })
                .into_any_element(),
            Content::Image { path, info } => v_flex()
                .size_full()
                .p_2()
                .gap_2()
                .child(
                    div().flex_1().min_h_0().child(
                        img(path.clone())
                            .size_full()
                            .object_fit(ObjectFit::Contain)
                            .with_fallback(|| {
                                div().child("Cannot decode image").into_any_element()
                            }),
                    ),
                )
                .when(!info.is_empty(), |d| d.child(details(info, cx)))
                .into_any_element(),
            Content::Info { info } => v_flex().p_4().child(details(info, cx)).into_any_element(),
            Content::Directory {
                dirs,
                files,
                files_size,
            } => v_flex()
                .p_4()
                .gap_1()
                .child(format!("{dirs} directories"))
                .child(format!(
                    "{files} files, {}",
                    fs_ops::format_size(*files_size)
                ))
                .child(note("Direct children only.".into()))
                .into_any_element(),
            Content::Binary { dump, truncated } => v_flex()
                .size_full()
                .child(
                    div()
                        .id("hex")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .p_2()
                        .font_family(theme.mono_font_family.clone())
                        .text_xs()
                        .whitespace_nowrap()
                        .children(dump.lines().map(|line| div().child(line.to_string()))),
                )
                .when(*truncated, |d| {
                    d.child(note("Showing the first 4 KiB.".into()))
                })
                .into_any_element(),
            Content::Markdown { text } => div()
                .id("markdown")
                .size_full()
                .overflow_y_scroll()
                .p_3()
                .child(TextView::markdown("markdown-view", text.clone()))
                .into_any_element(),
            Content::Table {
                header,
                rows,
                truncated,
            } => {
                let cell = |text: &String| {
                    div()
                        .w(px(140.))
                        .flex_none()
                        .px_2()
                        .py_0p5()
                        .truncate()
                        .child(text.clone())
                };
                v_flex()
                    .size_full()
                    .child(
                        div()
                            .id("table")
                            .flex_1()
                            .min_h_0()
                            .overflow_scroll()
                            .text_xs()
                            .child(
                                h_flex()
                                    .bg(theme.muted)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .children(header.iter().map(cell)),
                            )
                            .children(rows.iter().enumerate().map(|(i, row)| {
                                h_flex()
                                    .when(i % 2 == 1, |r| r.bg(theme.muted.opacity(0.4)))
                                    .children(row.iter().map(cell))
                            })),
                    )
                    .child(note(format!(
                        "{} rows{}",
                        rows.len(),
                        if *truncated {
                            if preview.kind == "Parquet" {
                                " (sample truncated)"
                            } else {
                                " (more not shown)"
                            }
                        } else {
                            ""
                        }
                    )))
                    .into_any_element()
            }
            Content::Archive {
                items,
                total,
                truncated,
            } => v_flex()
                .size_full()
                .child(
                    div()
                        .id("archive")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .p_2()
                        .text_xs()
                        .children(items.iter().map(|item| {
                            h_flex()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .when(item.is_dir, |d| d.text_color(theme.primary))
                                        .child(item.name.clone()),
                                )
                                .when(!item.is_dir, |r| {
                                    r.child(
                                        div()
                                            .flex_none()
                                            .text_color(theme.muted_foreground)
                                            .child(fs_ops::format_size(item.size)),
                                    )
                                })
                        })),
                )
                .child(note(format!(
                    "{total} entries{}",
                    if *truncated { ", first 2000 shown" } else { "" }
                )))
                .into_any_element(),
        }
    }
}

impl Render for PreviewPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (title, detail, link) = match &self.state {
            State::Ready(p) => (
                p.name.clone(),
                match p.content {
                    // A directory's own size is meaningless to users.
                    Content::Directory { .. } => p.kind.to_string(),
                    _ => format!("{} · {}", p.kind, fs_ops::format_size(p.size)),
                },
                p.link.tooltip_with_resolved(p.resolved_link.as_deref()),
            ),
            State::Loading => ("Loading…".into(), String::new(), None),
            State::Failed(_) | State::Empty => ("Preview".into(), String::new(), None),
        };
        let link_tip = link;
        let center = |text: String| {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(cx.theme().muted_foreground)
                .child(text)
        };
        let body = match &self.state {
            State::Ready(preview) => self.render_content(preview, cx),
            State::Loading => center(String::new()).into_any_element(),
            State::Empty => center("Nothing to preview".into()).into_any_element(),
            State::Failed(e) => center(e.clone()).into_any_element(),
        };

        v_flex()
            .size_full()
            .gap_1()
            .child(
                h_flex()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded(theme.radius)
                    .bg(theme.muted)
                    .child(div().flex_1().min_w_0().truncate().child(title))
                    // Where a symlink leads (and the resolved path), in full.
                    .when_some(link_tip, |d, tip| {
                        d.child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(tip.replace('\n', " → ")),
                        )
                    })
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(detail),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .rounded(theme.radius)
                    .border_1()
                    .border_color(theme.border)
                    .overflow_hidden()
                    .child(body),
            )
    }
}

/// Labelled details (EXIF, PDF info, media streams), one per line.
fn details(info: &[(String, String)], cx: &App) -> impl IntoElement {
    let muted = cx.theme().muted_foreground;
    v_flex()
        .flex_none()
        .gap_0p5()
        .text_xs()
        .children(info.iter().map(|(label, value)| {
            h_flex()
                .gap_2()
                .child(
                    div()
                        .w(px(90.))
                        .flex_none()
                        .text_color(muted)
                        .child(label.clone()),
                )
                .child(div().min_w_0().truncate().child(value.clone()))
        }))
}

#[cfg(test)]
mod tests {
    use super::PreviewTarget;
    use crate::location::Location as TypedLocation;
    use crate::remote::Location as RemoteLocation;
    use std::path::PathBuf;

    #[test]
    fn materialized_preview_identity_is_not_its_cache_path() {
        let path = PathBuf::from("/cache/fyle/remote/gdrive/report.docx");
        let first = PreviewTarget {
            path: Some(path.clone()),
            identity: Some(TypedLocation::Remote(
                RemoteLocation::root("gdrive")
                    .unwrap()
                    .child("report")
                    .unwrap(),
            )),
        };
        let other = PreviewTarget {
            path: Some(path),
            identity: Some(TypedLocation::Remote(
                RemoteLocation::root("onedrive")
                    .unwrap()
                    .child("report")
                    .unwrap(),
            )),
        };
        assert_ne!(first, other);
        let unavailable = PreviewTarget {
            path: None,
            identity: first.identity.clone(),
        };
        assert_ne!(first, unavailable);
    }
}
