//! Icon grid view of a [`Folder`]: cells with a large file-type icon and the
//! name below (image files show a thumbnail once it is loaded, see
//! `thumbnails.rs`). Geometry and cursor movement come from `grid.rs`, the
//! thumbnail scheduling from `thumb_queue.rs`; this file only draws. Rows are rendered lazily (`uniform_list`), so large folders
//! stay fast.

use std::cell::Cell;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, Size, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::context_menu::{self, ContextMenuBuilder};
use crate::fs_ops;
use crate::glyphs::glyph;
use crate::grid::{self, GAP, Layout, Move, PADDING};
use crate::icons;
use crate::model::{Folder, Row};
use crate::thumb_queue::{Key, Queue};
use crate::thumbnails::{self, SizeClass};

/// Thumbnails kept in memory, and decoded at the same time.
const THUMB_CAPACITY: usize = 300;
const THUMB_JOBS: usize = 3;

actions!(
    fyle_grid,
    [
        GridLeft,
        GridRight,
        GridUp,
        GridDown,
        GridPageUp,
        GridPageDown
    ]
);

/// Key context of the grid (it also keeps the panel's keys, see `keys.rs`).
pub const CONTEXT: &str = "IconGrid";

pub fn bind_keys(cx: &mut App) {
    let grid = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("left", GridLeft, grid),
        KeyBinding::new("right", GridRight, grid),
        KeyBinding::new("up", GridUp, grid),
        KeyBinding::new("down", GridDown, grid),
        KeyBinding::new("pageup", GridPageUp, grid),
        KeyBinding::new("pagedown", GridPageDown, grid),
    ]);
}

/// What the user did in the grid; the panel applies it to the model.
pub enum GridEvent {
    /// Cursor to cell `ix`.
    Select(usize),
    /// Cursor to cell `ix` and open it (double-click).
    Activate(usize),
}

pub struct IconGrid {
    folder: Entity<Folder>,
    /// The panel's focus handle (shared with the list), so focus and the
    /// panel key context survive switching views.
    focus: FocusHandle,
    icon_size: u32,
    active: bool,
    scroll: UniformListScrollHandle,
    /// Width and height of the grid area as last painted; the layout and
    /// the arrow keys depend on it, and it is not known before painting.
    area: Rc<Cell<(f32, f32)>>,
    /// The (cursor, columns) last scrolled into view, so scrolling with the
    /// wheel is not undone by the next repaint.
    scrolled_to: Option<(usize, usize, u64, usize)>,
    context_menu: Option<ContextMenuBuilder>,
    /// Decoded thumbnails and what is being loaded; None without a cache dir.
    thumbs: Queue<Arc<RenderImage>>,
    thumb_root: Option<PathBuf>,
    /// The window's scale factor as last rendered, for picking thumbnail sizes.
    scale: f32,
    /// The rows and layout the list asked for last, and whether a frame
    /// callback will act on them (the list also asks to measure one row).
    visible: Option<(Range<usize>, Layout)>,
    thumbs_scheduled: bool,
    _folder_observer: Subscription,
}

impl EventEmitter<GridEvent> for IconGrid {}

impl IconGrid {
    pub fn new(
        folder: Entity<Folder>,
        focus: FocusHandle,
        icon_size: u32,
        cx: &mut Context<Self>,
    ) -> Self {
        let observer = cx.observe(&folder, |this, _, cx| {
            this.thumbs.cancel_waiting();
            cx.notify();
        });
        // Free the atlas tiles of the thumbnails still held.
        cx.on_release(|this, cx| {
            for image in this.thumbs.drain() {
                cx.drop_image(image, None);
            }
        })
        .detach();
        Self {
            folder,
            focus,
            icon_size: grid::clamp_size(icon_size),
            active: false,
            scroll: UniformListScrollHandle::new(),
            area: Rc::new(Cell::new((0., 0.))),
            scrolled_to: None,
            context_menu: None,
            thumbs: Queue::new(THUMB_CAPACITY, THUMB_JOBS),
            thumb_root: thumbnails::default_root(),
            scale: 1.,
            visible: None,
            thumbs_scheduled: false,
            _folder_observer: observer,
        }
    }

    pub fn icon_size(&self) -> u32 {
        self.icon_size
    }

    pub fn set_icon_size(&mut self, size: u32, cx: &mut Context<Self>) {
        self.icon_size = grid::clamp_size(size);
        self.scrolled_to = None; // keep the cursor in view at the new size
        cx.notify();
    }

    pub fn set_active(&mut self, active: bool, cx: &mut Context<Self>) {
        self.active = active;
        cx.notify();
    }

    pub fn set_context_menu(&mut self, builder: ContextMenuBuilder) {
        self.context_menu = Some(builder);
    }

    /// Height of the two name lines under an icon, and the name's font size.
    fn label_metrics(cx: &App) -> (f32, f32, f32) {
        let text = f32::from(cx.theme().font_size) * 0.8;
        let line = (text * 1.25).round();
        (text, line, 2. * line)
    }

    fn layout(&self, cx: &App) -> (Layout, f32) {
        let (_, _, label) = Self::label_metrics(cx);
        let (width, height) = self.area.get();
        // Before the first paint the width is unknown: assume a typical panel.
        let width = if width > 0. { width } else { 560. };
        (Layout::new(width, self.icon_size, label), height)
    }

    fn move_cursor(&mut self, mv: Move, cx: &mut Context<Self>) {
        let (layout, height) = self.layout(cx);
        let folder = self.folder.read(cx);
        let target = grid::move_cursor(
            folder.cursor(),
            folder.entries().len(),
            layout.cols,
            layout.page_rows(height),
            mv,
        );
        if target != folder.cursor() {
            cx.emit(GridEvent::Select(target));
        }
    }

    /// The thumbnail identity of a row, if it can have one.
    fn thumb_key(&self, row: &Row, in_archive: bool) -> Option<Key> {
        let root = self.thumb_root.as_deref()?;
        let entry = &row.entry;
        // Remote files are not on the disk to be thumbnailed.
        if in_archive || entry.is_dir || entry.is_parent || crate::remote::is_remote(&entry.path) {
            return None;
        }
        let mtime = entry.modified?.duration_since(UNIX_EPOCH).ok()?.as_secs() as i64;
        thumbnails::eligible(&entry.path, entry.size, root).then(|| Key {
            path: entry.path.clone(),
            mtime,
            class: SizeClass::for_pixels((self.icon_size as f32 * self.scale).ceil() as u32),
        })
    }

    /// Asks for the thumbnails of the cells in `rows` (visible rows first,
    /// then one row before and after) and starts loading.
    fn want_thumbnails(&mut self, rows: Range<usize>, layout: Layout, cx: &mut Context<Self>) {
        let folder = self.folder.read(cx);
        let (count, in_archive) = (folder.entries().len(), folder.archive().is_some());
        let around = rows.start.saturating_sub(1)..(rows.end + 1).min(layout.rows(count));
        let order = rows.clone().chain(around.filter(|row| !rows.contains(row)));
        let keys: Vec<Key> = order
            .flat_map(|row| layout.cells(row, count))
            .filter_map(|ix| self.thumb_key(folder.entries().get(ix)?, in_archive))
            .collect();
        self.thumbs.want(keys);
        self.start_thumbnails(cx);
    }

    /// Starts the waiting loads the concurrency bound allows, off the UI
    /// thread; each finished load notifies the grid and starts the next.
    fn start_thumbnails(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.thumb_root.clone() else {
            return;
        };
        for key in self.thumbs.start() {
            let (root, job) = (root.clone(), key.clone());
            cx.spawn(async move |this, cx| {
                let image = cx
                    .background_spawn(async move {
                        Some(
                            thumbnails::thumbnail(&root, &job.path, job.class)
                                .map(|image| Arc::new(render_image(image))),
                        )
                    })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    match image {
                        Some(Ok(image)) => Self::release(this.thumbs.finish(key, Some(image)), cx),
                        Some(Err(thumbnails::Skip::Io)) => this.thumbs.abandon(&key),
                        _ => Self::release(this.thumbs.finish(key, None), cx),
                    }
                    this.start_thumbnails(cx);
                    cx.notify();
                });
            })
            .detach();
        }
    }

    /// Frees the GPU copies of thumbnails that left the cache.
    fn release(evicted: Vec<Arc<RenderImage>>, cx: &mut App) {
        for image in evicted {
            cx.drop_image(image, None);
        }
    }

    /// Remembers the rows drawn and, once per frame, requests thumbnails for
    /// the last ones asked for (the real visible range, which the list asks
    /// for after measuring a single row).
    fn schedule_thumbnails(
        &mut self,
        rows: Range<usize>,
        layout: Layout,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        self.visible = Some((rows, layout));
        if self.thumbs_scheduled {
            return;
        }
        self.thumbs_scheduled = true;
        let this = cx.weak_entity();
        window.on_next_frame(move |_, cx| {
            let _ = this.update(cx, |this, cx| {
                this.thumbs_scheduled = false;
                if let Some((rows, layout)) = this.visible.take() {
                    this.want_thumbnails(rows, layout, cx);
                }
            });
        });
    }

    /// The picture of a cell: the thumbnail when loaded, else the file-type
    /// glyph; the one place that decides what the picture of an entry is.
    fn cell_visual(&self, row: &Row, cx: &App) -> AnyElement {
        let entry = &row.entry;
        let in_archive = self.folder.read(cx).archive().is_some();
        let thumbnail = self
            .thumb_key(row, in_archive)
            .and_then(|key| self.thumbs.get(&key).cloned());
        let expanded = false; // tree mode does not apply to the grid
        let icon = glyph(icons::icon_for(
            &entry.name,
            entry.is_dir,
            entry.is_parent,
            expanded,
        ));
        let size = self.icon_size as f32;
        let badge = match self.icon_size {
            ..64 => Size::XSmall,
            64..128 => Size::Small,
            _ => Size::Medium,
        };
        let picture = match thumbnail {
            Some(image) => {
                // Size the image to its fitted box: left to the layout,
                // gpui's aspect ratio stretched portrait images past the
                // box height and the clip cropped them.
                let frame = image.size(0);
                let (w, h) = crate::grid::fit(frame.width.0 as f32, frame.height.0 as f32, size);
                div()
                    .size(px(size))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(img(ImageSource::Render(image)).w(px(w)).h(px(h)))
                    .into_any_element()
            }
            None => Icon::new(icon).size(px(size * 0.8)).into_any_element(),
        };
        h_flex()
            .size(px(size))
            .flex_none()
            .items_center()
            .justify_center()
            .child(crate::link_badge::badged_sized(
                picture,
                &entry.link,
                badge,
                cx,
            ))
            .into_any_element()
    }

    fn tooltip_text(row: &Row, folder: &Folder) -> String {
        let entry = &row.entry;
        let mut text = entry.name.clone();
        if !entry.is_parent {
            let size = if entry.is_dir {
                folder.dir_size(&entry.path).map(|size| size.label())
            } else {
                Some(fs_ops::format_size(entry.size))
            };
            if let Some(size) = size {
                text += &format!("\n{size}");
            }
            if let Some(modified) = entry.modified {
                text += &format!("\n{}", fs_ops::format_time(modified));
            }
        }
        if let Some(link) = entry.link.tooltip(&entry.path) {
            text += &format!("\n{link}");
        }
        text
    }

    fn render_cell(&self, ix: usize, layout: Layout, cx: &mut Context<Self>) -> AnyElement {
        let folder = self.folder.read(cx);
        let Some(row) = folder.entries().get(ix) else {
            return div().into_any_element();
        };
        let entry = &row.entry;
        let theme = cx.theme();
        let marked = folder.is_marked(&entry.path);
        let at_cursor = folder.cursor() == ix;
        let color = if marked {
            theme.danger
        } else if entry.link.is_broken() {
            theme.muted_foreground
        } else if entry.is_dir {
            theme.primary
        } else {
            theme.foreground
        };
        let (text_size, line_height, label_height) = Self::label_metrics(cx);
        let tip = Self::tooltip_text(row, folder);
        let name = entry.name.clone();
        let (background, border, check_color) = (
            theme.list_active,
            theme.list_active_border,
            theme.danger_foreground,
        );
        let marked_border = theme.danger;
        let (focus, folder_entity) = (self.focus.clone(), self.folder.clone());
        let builder = self.context_menu.clone();
        let visual = self.cell_visual(row, cx);

        div()
            .id(("cell", ix))
            .relative()
            .flex_none()
            .w(px(layout.cell_width))
            .h(px(layout.cell_height))
            .p(px(PADDING))
            .rounded(cx.theme().radius)
            .border_2()
            .border_color(transparent_black())
            .when(at_cursor, |d| {
                d.bg(background)
                    .when(self.active, |d| d.border_color(border))
            })
            // A marked cell shows an outline and a check, not only a colour.
            .when(marked, |d| {
                d.border_color(marked_border).child(
                    div()
                        .absolute()
                        .top(px(2.))
                        .left(px(2.))
                        .rounded_full()
                        .bg(marked_border)
                        .text_color(check_color)
                        .child(Icon::new(Lucide::Check).with_size(Size::Small)),
                )
            })
            .flex()
            .flex_col()
            .items_center()
            .gap_1()
            .text_color(color)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.focus.focus(window, cx);
                    cx.emit(if event.click_count >= 2 {
                        GridEvent::Activate(ix)
                    } else {
                        GridEvent::Select(ix)
                    });
                }),
            )
            // Focus before the menu captures where to return on dismissal.
            .capture_any_mouse_down(move |event, window, cx| {
                if event.button == MouseButton::Right {
                    focus.focus(window, cx);
                }
            })
            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            .child(visual)
            .child(
                div()
                    .w_full()
                    .h(px(label_height))
                    .overflow_hidden()
                    .text_size(px(text_size))
                    .line_height(px(line_height))
                    .text_center()
                    .line_clamp(2)
                    .text_ellipsis()
                    .child(name),
            )
            .context_menu(move |menu, window, cx| {
                context_menu::file_menu(&folder_entity, ix, builder.as_ref(), menu, window, cx)
            })
            .into_any_element()
    }

    fn render_rows(
        &mut self,
        rows: Range<usize>,
        layout: Layout,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        self.scale = window.scale_factor();
        let count = self.folder.read(cx).entries().len();
        self.schedule_thumbnails(rows.clone(), layout, window, cx);
        rows.map(|row| {
            let cells: Vec<_> = layout
                .cells(row, count)
                .map(|ix| self.render_cell(ix, layout, cx))
                .collect();
            h_flex()
                .h(px(layout.row_height()))
                .items_start()
                .gap(px(GAP))
                .children(cells)
                .into_any_element()
        })
        .collect()
    }
}

impl Focusable for IconGrid {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for IconGrid {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (layout, _) = self.layout(cx);
        let folder = self.folder.read(cx);
        let (cursor, count) = (folder.cursor(), folder.entries().len());
        // The listing changing (filter, other folder, refresh) re-scrolls too.
        let key = (cursor, layout.cols, folder.generation(), count);
        if self.scrolled_to != Some(key) {
            self.scrolled_to = Some(key);
            self.scroll
                .scroll_to_item(layout.row_of(cursor), ScrollStrategy::Nearest);
        }
        let (area, view) = (self.area.clone(), cx.weak_entity());
        v_flex()
            .id("icon-grid")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .on_action(cx.listener(|this, _: &GridLeft, _, cx| this.move_cursor(Move::Left, cx)))
            .on_action(cx.listener(|this, _: &GridRight, _, cx| this.move_cursor(Move::Right, cx)))
            .on_action(cx.listener(|this, _: &GridUp, _, cx| this.move_cursor(Move::Up, cx)))
            .on_action(cx.listener(|this, _: &GridDown, _, cx| this.move_cursor(Move::Down, cx)))
            .on_action(
                cx.listener(|this, _: &GridPageUp, _, cx| this.move_cursor(Move::PageUp, cx)),
            )
            .on_action(
                cx.listener(|this, _: &GridPageDown, _, cx| this.move_cursor(Move::PageDown, cx)),
            )
            .child(
                div().relative().flex_1().min_h_0().p_1().child(
                    div()
                        .relative()
                        .size_full()
                        .child(
                            // Measures the area; a changed size repaints the
                            // grid with the new number of columns.
                            canvas(
                                move |bounds, _, cx| {
                                    let size = (
                                        f32::from(bounds.size.width),
                                        f32::from(bounds.size.height),
                                    );
                                    let old = area.get();
                                    if (old.0 - size.0).abs() > 0.5 || (old.1 - size.1).abs() > 0.5
                                    {
                                        area.set(size);
                                        let _ = view.update(cx, |_, cx| cx.notify());
                                    }
                                },
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .size_full(),
                        )
                        .child(
                            uniform_list(
                                "icon-rows",
                                layout.rows(count),
                                cx.processor(move |this, rows, window, cx| {
                                    this.render_rows(rows, layout, window, cx)
                                }),
                            )
                            .track_scroll(&self.scroll)
                            .size_full(),
                        ),
                ),
            )
    }
}

/// A thumbnail as gpui draws it: BGRA pixels in a single frame.
fn render_image(mut image: image::RgbaImage) -> RenderImage {
    for pixel in image.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    RenderImage::new(vec![image::Frame::new(image)])
}
