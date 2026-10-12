//! Quick-view pane: shows a [`Preview`] of a path. Loading runs in the
//! background; each [`Content`] variant has one render arm here.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Editor, EditorState, Position};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::text::TextView;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::fs_ops;
use crate::preview::session::{PreviewSession, Request, State, TargetChange};
use crate::preview::{Content, Preview, Registry};

struct LoadedPreview {
    preview: Preview,
    notebook_images: HashMap<(usize, usize), Arc<RenderImage>>,
}

/// How long the cursor must rest on an entry before its preview loads.
const LOAD_DELAY: Duration = Duration::from_millis(60);

pub struct PreviewPane {
    registry: Arc<Registry>,
    session: PreviewSession,
    /// Reused read-only editor for text content.
    editor: Entity<EditorState>,
    /// Variable-height virtual list state for notebook cells.
    notebook_list: ListState,
    /// Raster images prepared off the UI thread for notebook outputs.
    notebook_images: Arc<HashMap<(usize, usize), Arc<RenderImage>>>,
    /// Scroll position for both axes of a table preview.
    table_scroll: ScrollHandle,
    _pdf_render: Task<()>,
    _sheet_load: Task<()>,
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
            session: PreviewSession::default(),
            editor,
            notebook_list: ListState::new(0, ListAlignment::Top, px(160.)),
            notebook_images: Arc::new(HashMap::new()),
            table_scroll: ScrollHandle::new(),
            _pdf_render: Task::ready(()),
            _sheet_load: Task::ready(()),
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
        let change = self.session.set_target(path, identity, line);
        self.apply_target_change(change, window, cx);
    }

    fn apply_target_change(
        &mut self,
        change: TargetChange,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let request = match change {
            TargetChange::Unchanged => return,
            TargetChange::Reveal => {
                self.reveal_line(window, cx);
                return;
            }
            TargetChange::Reset(request) => request,
        };
        self.table_scroll.set_offset(point(px(0.), px(0.)));
        self.release_notebook_images(window, cx);
        self._pdf_render = Task::ready(());
        self._sheet_load = Task::ready(());
        self._reveal = Task::ready(());
        let Some(request) = request else {
            self._load = Task::ready(());
            cx.notify();
            return;
        };
        let path = request.path.clone();
        let registry = self.registry.clone();
        self._load = cx.spawn_in(window, async move |this, cx| {
            // Holding an arrow key moves the cursor faster than this; the
            // next target drops the task, so only the entry it stops on loads.
            cx.background_executor().timer(LOAD_DELAY).await;
            let result = cx
                .background_spawn(async move {
                    let preview = registry.preview(&path)?;
                    let images = match &preview.content {
                        Content::Notebook(notebook) => {
                            crate::notebook_view::prepare_images(notebook)
                        }
                        _ => HashMap::new(),
                    };
                    Ok(LoadedPreview {
                        preview,
                        notebook_images: images,
                    })
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.show(&request, result, window, cx)
            });
        });
        cx.notify();
    }

    /// Loads the current target again (e.g. after it was edited).
    pub fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let change = self.session.reload();
        self.apply_target_change(change, window, cx);
    }

    /// Moves the text view to the requested line without taking focus. Again
    /// after a moment too: scrolling needs the editor's layout, which new
    /// content only has once drawn.
    fn reveal_line(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(line) = self.session.line() else {
            self._reveal = Task::ready(());
            return;
        };
        if !matches!(self.session.state(), State::Ready(p) if matches!(p.content, Content::Text { .. }))
        {
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
        request: &Request,
        result: Result<LoadedPreview, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (result, images) = match result {
            Ok(loaded) => (Ok(loaded.preview), loaded.notebook_images),
            Err(error) => (Err(error), HashMap::new()),
        };
        if !self.session.complete(request, result) {
            return;
        }
        self.release_notebook_images(window, cx);
        self.notebook_images = Arc::new(images);
        if let State::Ready(preview) = self.session.state() {
            if let Content::Notebook(notebook) = &preview.content {
                self.notebook_list =
                    ListState::new(notebook.cells.len(), ListAlignment::Top, px(240.));
            }
            if let Content::Text { text, language, .. } = &preview.content {
                self.editor.update(cx, |editor, cx| {
                    editor.set_highlighter(language.unwrap_or("text"), cx);
                    editor.set_value(text.clone(), window, cx);
                });
            }
        }
        if let Some(request) = self.session.render_pdf() {
            self.render_pdf_page(request, window, cx);
        }
        self.reveal_line(window, cx);
        cx.notify();
    }

    /// Renders only the selected PDF page in the background.
    fn render_pdf_page(&mut self, request: Request, window: &mut Window, cx: &mut Context<Self>) {
        let path = request.path.clone();
        let page = request.index;
        self._pdf_render = cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move { crate::preview::render_page(&path, page) })
                .await
                .map_err(|error| error.to_string());
            let _ = this.update_in(cx, |this, _, cx| {
                if this.session.complete_pdf(&request, result) {
                    cx.notify();
                }
            });
        });
        cx.notify();
    }

    fn change_pdf_page(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(request) = self.session.change_pdf_page(delta) {
            self.render_pdf_page(request, window, cx);
        }
    }

    /// Shows another sheet of the workbook, read in the background.
    fn change_sheet(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(request) = self.session.change_sheet(delta) else {
            return;
        };
        let path = request.path.clone();
        let index = request.index;
        self.table_scroll.set_offset(point(px(0.), px(0.)));
        self._sheet_load = cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move { crate::preview::read_sheet(&path, index) })
                .await
                .map_err(|error| error.to_string());
            let _ = this.update_in(cx, |this, _, cx| {
                if this.session.complete_sheet(&request, result) {
                    cx.notify();
                }
            });
        });
        cx.notify();
    }

    fn release_notebook_images(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for image in self.notebook_images.values() {
            cx.drop_image(image.clone(), Some(window));
        }
        self.notebook_images = Arc::new(HashMap::new());
    }

    /// A scrollable table with a footer note; `row_numbers` narrows the first
    /// column, which then holds spreadsheet row numbers.
    fn render_table(
        &self,
        header: &[String],
        rows: &[Vec<String>],
        footer: String,
        row_numbers: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let columns = rows
            .iter()
            .map(Vec::len)
            .max()
            .unwrap_or(0)
            .max(header.len());
        let first_width = if row_numbers { 48. } else { 140. };
        let table_width = px(first_width + 140. * columns.saturating_sub(1) as f32);
        let scroll = self.table_scroll.clone();
        let cell = |(i, text): (usize, &String)| {
            div()
                .w(px(if i == 0 { first_width } else { 140. }))
                .flex_none()
                .px_2()
                .py_0p5()
                .truncate()
                .when(row_numbers && i == 0, |d| {
                    d.text_color(theme.muted_foreground)
                })
                .child(text.clone())
        };
        v_flex()
            .size_full()
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id("table-scroll-area")
                            .size_full()
                            .overflow_scroll()
                            .track_scroll(&scroll)
                            .text_xs()
                            .child(
                                v_flex()
                                    .w(table_width)
                                    .flex_none()
                                    .child(
                                        h_flex()
                                            .w(table_width)
                                            .flex_none()
                                            .bg(theme.muted)
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .children(header.iter().enumerate().map(cell)),
                                    )
                                    .children(rows.iter().enumerate().map(|(i, row)| {
                                        h_flex()
                                            .w(table_width)
                                            .flex_none()
                                            .when(i % 2 == 1, |r| r.bg(theme.muted.opacity(0.4)))
                                            .children(row.iter().enumerate().map(cell))
                                    })),
                            ),
                    )
                    .vertical_scrollbar(&scroll)
                    .horizontal_scrollbar(&scroll),
            )
            .child(
                div()
                    .px_2()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(footer),
            )
            .into_any_element()
    }

    fn render_content(&self, preview: &Preview, cx: &Context<Self>) -> AnyElement {
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
            Content::Pdf { pages, .. } => {
                let page = self.session.pdf_page();
                let (image, error) = match self.session.pdf_result() {
                    Some(Ok(path)) => (Some(path.clone()), None),
                    Some(Err(error)) => (None, Some(error.clone())),
                    None => (None, None),
                };
                let controls = h_flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .child(
                        Button::new("pdf-previous")
                            .label("Previous")
                            .disabled(page <= 1)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.change_pdf_page(-1, window, cx)
                            })),
                    )
                    .child(format!("Page {page} of {pages}"))
                    .child(
                        Button::new("pdf-next")
                            .label("Next")
                            .disabled(page >= *pages)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.change_pdf_page(1, window, cx)
                            })),
                    );
                v_flex()
                    .size_full()
                    .p_2()
                    .gap_2()
                    .child(controls)
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .when_some(image, |view, path| {
                                view.child(
                                    img(path)
                                        .size_full()
                                        .object_fit(ObjectFit::Contain)
                                        .with_fallback(|| {
                                            div().child("Cannot decode PDF page").into_any_element()
                                        }),
                                )
                            })
                            .when_some(error, |view, error| {
                                view.child(div().text_color(theme.muted_foreground).child(error))
                            })
                            .when(self.session.pdf_result().is_none(), |view| {
                                view.child(
                                    div()
                                        .text_color(theme.muted_foreground)
                                        .child("Rendering page…"),
                                )
                            }),
                    )
                    .into_any_element()
            }
            Content::Info { info } => div()
                .size_full()
                .overflow_y_scrollbar()
                .p_4()
                .child(details(info, cx))
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
                        .overflow_y_scrollbar()
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
                .overflow_y_scrollbar()
                .p_3()
                .child(
                    TextView::markdown("markdown-view", text.clone()).font_family(
                        crate::themes::rich_text_font_for_app(&theme.font_family, cx),
                    ),
                )
                .into_any_element(),
            Content::Notebook(notebook) => crate::notebook_view::render(
                notebook.clone(),
                self.notebook_list.clone(),
                self.notebook_images.clone(),
                cx,
            ),
            Content::Table {
                header,
                rows,
                truncated,
            } => {
                let footer = format!(
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
                );
                self.render_table(header, rows, footer, false, cx)
            }
            Content::Spreadsheet { sheets, .. } => {
                let index = self.session.sheet_index();
                let count = sheets.len();
                let name = sheets.get(index).cloned().unwrap_or_default();
                let controls =
                    h_flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .gap_3()
                        .p_2()
                        .child(
                            Button::new("sheet-previous")
                                .label("Previous")
                                .disabled(index == 0)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.change_sheet(-1, window, cx)
                                })),
                        )
                        .child(format!("{name} ({} of {count})", index + 1))
                        .child(
                            Button::new("sheet-next")
                                .label("Next")
                                .disabled(index + 1 >= count)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.change_sheet(1, window, cx)
                                })),
                        );
                let body = match self.session.sheet() {
                    Some(Ok(sheet)) => {
                        self.render_table(&sheet.header, &sheet.rows, sheet.summary(), true, cx)
                    }
                    Some(Err(error)) => div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(theme.muted_foreground)
                        .child(error.clone())
                        .into_any_element(),
                    None => div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(theme.muted_foreground)
                        .child("Loading sheet…")
                        .into_any_element(),
                };
                v_flex()
                    .size_full()
                    .child(controls)
                    .child(div().flex_1().min_h_0().child(body))
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
                        .overflow_y_scrollbar()
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
        let (title, detail, link) = match self.session.state() {
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
        let body = match self.session.state() {
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
fn details(info: &[(String, String)], cx: &Context<PreviewPane>) -> impl IntoElement {
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
