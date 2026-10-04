//! Quick-view pane: shows a [`Preview`] of a path. Loading runs in the
//! background; each [`Content`] variant has one render arm here.

use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::component::input::{Editor, EditorState};
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

pub struct PreviewPane {
    registry: Arc<Registry>,
    target: Option<PathBuf>,
    state: State,
    /// Reused read-only editor for text content.
    editor: Entity<EditorState>,
    _load: Task<()>,
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
            state: State::Empty,
            editor,
            _load: Task::ready(()),
        }
    }

    /// Shows `path`; a new target cancels the previous load.
    pub fn set_target(
        &mut self,
        path: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if path == self.target {
            return;
        }
        self.target = path.clone();
        let Some(path) = path else {
            self.state = State::Empty;
            self._load = Task::ready(());
            cx.notify();
            return;
        };
        self.state = State::Loading;
        let registry = self.registry.clone();
        self._load = cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move { registry.preview(&path) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| this.show(result, window, cx));
        });
        cx.notify();
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
            Content::Image { path } => div()
                .size_full()
                .p_2()
                .child(
                    img(path.clone())
                        .size_full()
                        .object_fit(ObjectFit::Contain)
                        .with_fallback(|| div().child("Cannot decode image").into_any_element()),
                )
                .into_any_element(),
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
        }
    }
}

impl Render for PreviewPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (title, detail) = match &self.state {
            State::Ready(p) => (
                p.name.clone(),
                match p.content {
                    // A directory's own size is meaningless to users.
                    Content::Directory { .. } => p.kind.to_string(),
                    _ => format!("{} · {}", p.kind, fs_ops::format_size(p.size)),
                },
            ),
            State::Loading => ("Loading…".into(), String::new()),
            State::Failed(_) | State::Empty => ("Preview".into(), String::new()),
        };
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
