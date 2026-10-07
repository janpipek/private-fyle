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
use crate::preview::{Content, Preview, Registry, Sheet};

enum State {
    Empty,
    Loading,
    Ready(Preview),
    Failed(String),
}

struct LoadedPreview {
    preview: Preview,
    notebook_images: HashMap<(usize, usize), Arc<RenderImage>>,
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
    /// Variable-height virtual list state for notebook cells.
    notebook_list: ListState,
    /// Raster images prepared off the UI thread for notebook outputs.
    notebook_images: Arc<HashMap<(usize, usize), Arc<RenderImage>>>,
    /// Scroll position for both axes of a table preview.
    table_scroll: ScrollHandle,
    pdf_page: usize,
    pdf_generation: u64,
    pdf_image: Option<PathBuf>,
    pdf_error: Option<String>,
    _pdf_render: Task<()>,
    /// Selected sheet (0-based) of a spreadsheet and its sample, `None` while loading.
    sheet_index: usize,
    sheet_generation: u64,
    sheet: Option<Result<Sheet, String>>,
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
            target: None,
            line: None,
            state: State::Empty,
            editor,
            notebook_list: ListState::new(0, ListAlignment::Top, px(160.)),
            notebook_images: Arc::new(HashMap::new()),
            table_scroll: ScrollHandle::new(),
            pdf_page: 1,
            pdf_generation: 0,
            pdf_image: None,
            pdf_error: None,
            _pdf_render: Task::ready(()),
            sheet_index: 0,
            sheet_generation: 0,
            sheet: None,
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
        self.table_scroll.set_offset(point(px(0.), px(0.)));
        self.release_notebook_images(window, cx);
        self.pdf_page = 1;
        self.pdf_generation = self.pdf_generation.wrapping_add(1);
        self.pdf_image = None;
        self.pdf_error = None;
        self._pdf_render = Task::ready(());
        self.sheet_index = 0;
        self.sheet_generation = self.sheet_generation.wrapping_add(1);
        self.sheet = None;
        self._sheet_load = Task::ready(());
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
        result: Result<LoadedPreview, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state = match result {
            Ok(loaded) => {
                let preview = loaded.preview;
                self.release_notebook_images(window, cx);
                self.notebook_images = Arc::new(loaded.notebook_images);
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
                if let Content::Spreadsheet { first, .. } = &preview.content {
                    self.sheet = Some(Ok(Sheet::clone(first)));
                }
                State::Ready(preview)
            }
            Err(e) => State::Failed(e),
        };
        let pdf_path = match &self.state {
            State::Ready(preview) => match &preview.content {
                Content::Pdf { path, .. } => Some(path.clone()),
                _ => None,
            },
            _ => None,
        };
        if let Some(path) = pdf_path {
            self.render_pdf_page(path, window, cx);
        }
        self.reveal_line(window, cx);
        cx.notify();
    }

    /// Renders only the selected PDF page in the background.
    fn render_pdf_page(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let page = self.pdf_page;
        self.pdf_generation = self.pdf_generation.wrapping_add(1);
        let generation = self.pdf_generation;
        self.pdf_image = None;
        self.pdf_error = None;
        self._pdf_render = cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move { crate::preview::render_page(&path, page) })
                .await
                .map_err(|error| error.to_string());
            let _ = this.update_in(cx, |this, _, cx| {
                if this.pdf_page == page && this.pdf_generation == generation {
                    match result {
                        Ok(path) => this.pdf_image = Some(path),
                        Err(error) => this.pdf_error = Some(error),
                    }
                    cx.notify();
                }
            });
        });
        cx.notify();
    }

    fn change_pdf_page(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let Some((path, pages)) = (match &self.state {
            State::Ready(preview) => match &preview.content {
                Content::Pdf { path, pages, .. } => Some((path.clone(), *pages)),
                _ => None,
            },
            _ => None,
        }) else {
            return;
        };
        let page = self.pdf_page.saturating_add_signed(delta).clamp(1, pages);
        if page != self.pdf_page {
            self.pdf_page = page;
            self.render_pdf_page(path, window, cx);
        }
    }

    /// Shows another sheet of the workbook, read in the background.
    fn change_sheet(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let Some((path, count)) = (match &self.state {
            State::Ready(preview) => match &preview.content {
                Content::Spreadsheet { path, sheets, .. } => Some((path.clone(), sheets.len())),
                _ => None,
            },
            _ => None,
        }) else {
            return;
        };
        let index = self
            .sheet_index
            .saturating_add_signed(delta)
            .min(count.saturating_sub(1));
        if index == self.sheet_index {
            return;
        }
        self.sheet_index = index;
        self.sheet_generation = self.sheet_generation.wrapping_add(1);
        let generation = self.sheet_generation;
        self.sheet = None;
        self.table_scroll.set_offset(point(px(0.), px(0.)));
        self._sheet_load = cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move { crate::preview::read_sheet(&path, index) })
                .await
                .map_err(|error| error.to_string());
            let _ = this.update_in(cx, |this, _, cx| {
                if this.sheet_generation == generation {
                    this.sheet = Some(result);
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
                let page = self.pdf_page;
                let image = self.pdf_image.clone();
                let error = self.pdf_error.clone();
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
                            .when(
                                self.pdf_image.is_none() && self.pdf_error.is_none(),
                                |view| {
                                    view.child(
                                        div()
                                            .text_color(theme.muted_foreground)
                                            .child("Rendering page…"),
                                    )
                                },
                            ),
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
                .child(TextView::markdown("markdown-view", text.clone()))
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
                let index = self.sheet_index;
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
                let body = match &self.sheet {
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
