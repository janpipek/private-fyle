//! Properties perspective: schedules bounded inspection and renders semantic groups.

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::*;

use crate::location::Location;
use crate::properties::{
    GroupStatus, Inspection, PropertiesSession, PropertyGroup, PropertyValue, Request, Section,
    Selection, State,
};

/// Renders filesystem and format properties for the inspector's selected entry.
pub struct PropertiesPane {
    session: PropertiesSession,
    _load: Task<()>,
    running: bool,
    pending: Option<Request>,
}

impl PropertiesPane {
    /// Creates an empty Properties perspective.
    pub fn new() -> Self {
        Self {
            session: PropertiesSession::default(),
            _load: Task::ready(()),
            running: false,
            pending: None,
        }
    }

    /// Selects a new entry and starts inspection on a background worker.
    pub fn set_target(&mut self, selection: Option<Selection>, cx: &mut Context<Self>) {
        if let Some(request) = self.session.set_target(selection) {
            self.pending = Some(request);
        } else if matches!(self.session.state(), State::Empty) {
            self.pending = None;
        }
        self.start_pending(cx);
        cx.notify();
    }

    /// Invalidates the current inspection after a refresh or editor save.
    pub fn invalidate(&mut self, cx: &mut Context<Self>) {
        self.pending = self.session.invalidate();
        self.start_pending(cx);
        cx.notify();
    }

    fn start_pending(&mut self, cx: &mut Context<Self>) {
        if self.running {
            return;
        }
        let Some(request) = self.pending.take() else {
            return;
        };
        self.running = true;
        let worker_request = request.clone();
        self._load = cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    crate::properties::inspect(&worker_request.selection, &worker_request.cancel)
                        .map_err(|error| {
                            format!("{}: {error}", worker_request.selection.entry.path.display())
                        })
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.session.complete(&request, result) {
                    cx.notify();
                }
                this.running = false;
                this.start_pending(cx);
            });
        });
    }
}

impl Render for PropertiesPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.session.state() {
            State::Empty => message("Select a file or directory", cx),
            State::Loading(request) => message(&request.selection.entry.name, cx),
            State::Failed { error, .. } => message(error, cx),
            State::Ready { inspection, .. } => render_inspection(inspection, cx),
        };
        div().size_full().overflow_y_scrollbar().child(content)
    }
}

fn message(text: &str, cx: &App) -> AnyElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_color(cx.theme().muted_foreground)
        .child(text.to_owned())
        .into_any_element()
}

fn render_inspection(inspection: &Inspection, cx: &App) -> AnyElement {
    let listing = &inspection.listing;
    let mut body = v_flex().w_full().gap_2().p_2();
    if let Location::Local(path) = &inspection.location {
        if let Some(filesystem) =
            render_filesystem(path.as_path(), listing, &inspection.filesystem, cx)
        {
            body = body.child(filesystem);
        }
    } else {
        body = body.child(render_listing(listing, &inspection.location, cx));
    }
    if inspection.exif.is_visible()
        && let Some(exif) = render_exif(&inspection.exif, cx)
    {
        body = body.child(exif);
    }
    for group in &inspection.additional_groups {
        if group.is_visible() {
            body = body.child(render_group(group, cx));
        }
    }
    body.into_any_element()
}

fn render_listing(
    listing: &crate::properties::ListingFacts,
    location: &Location,
    cx: &App,
) -> AnyElement {
    let mut section = v_flex()
        .w_full()
        .gap_1()
        .child(section_title("Source", cx))
        .child(field("Name", &listing.name, cx))
        .child(field(
            "Location",
            &location.panel_path().display().to_string(),
            cx,
        ))
        .child(field(
            "Type",
            if listing.is_dir { "Directory" } else { "File" },
            cx,
        ));
    if let Some(size) = listing.size {
        section = section.child(field("Size", &format_size(size), cx));
    }
    if let Some(modified) = listing.modified {
        section = section.child(field("Modified", &crate::fs_ops::format_time(modified), cx));
    }
    if let Some(link) = listing.link.tooltip_with_resolved(None) {
        section = section.child(field("Link", &link, cx));
    }
    section.into_any_element()
}

fn render_group(group: &PropertyGroup, cx: &App) -> AnyElement {
    let mut content = v_flex()
        .w_full()
        .gap_1()
        .child(section_title(&humanize(&group.id.0), cx));
    match &group.status {
        GroupStatus::Available => {}
        GroupStatus::Unavailable(reason) => {
            content = content.child(field("Status", reason, cx));
        }
        GroupStatus::Failed(error) => {
            content = content.child(field("Status", &format!("Unavailable: {error}"), cx));
        }
        GroupStatus::Truncated { reason } => {
            content = content.child(field("Status", reason, cx));
        }
    }
    for property_field in &group.fields {
        content = content.child(field(
            &humanize(&property_field.id),
            &format_value(&property_field.value),
            cx,
        ));
    }
    for nested in &group.groups {
        if nested.is_visible() {
            content = content.child(render_group(nested, cx));
        }
    }
    for table in &group.tables {
        if table.columns.is_empty() && table.rows.is_empty() {
            continue;
        }
        content = content.child(section_title(&humanize(&table.id), cx));
        let widths = table
            .columns
            .iter()
            .map(|column| match column.as_str() {
                "path" => 160.0,
                "uncompressed_bytes" => 164.0,
                "compressed_bytes" | "definition_level" | "repetition_level" => 148.0,
                "rows" => 100.0,
                "index" => 80.0,
                _ => 132.0,
            })
            .collect::<Vec<_>>();
        let table_width = px(widths.iter().sum::<f32>());
        let header =
            h_flex()
                .w(table_width)
                .flex_none()
                .children(table.columns.iter().zip(&widths).map(|(column, width)| {
                    div()
                        .w(px(*width))
                        .flex_none()
                        .px_1()
                        .text_xs()
                        .font_semibold()
                        .truncate()
                        .child(humanize(column))
                }));
        let rows = v_flex()
            .w(table_width)
            .flex_none()
            .children(table.rows.iter().map(|row| {
                h_flex()
                    .w(table_width)
                    .flex_none()
                    .children(row.iter().zip(&widths).map(|(value, width)| {
                        div()
                            .w(px(*width))
                            .flex_none()
                            .px_1()
                            .text_sm()
                            .truncate()
                            .child(format_value(value))
                    }))
            }));
        content = content.child(
            div()
                .relative()
                .w_full()
                .overflow_x_scrollbar()
                .id(format!("properties-table-{}-{}", group.id.0, table.id))
                .child(
                    v_flex()
                        .w(table_width)
                        .flex_none()
                        .child(header)
                        .child(rows),
                ),
        );
    }
    content.into_any_element()
}

fn render_filesystem(
    path: &std::path::Path,
    listing: &crate::properties::ListingFacts,
    value: &Section<crate::properties::FilesystemFacts>,
    cx: &App,
) -> Option<AnyElement> {
    match value {
        Section::Available(value) => Some(render_filesystem_facts(path, listing, value, cx)),
        Section::Unavailable(_) => None,
        Section::Failed(error) => Some(render_filesystem_fallback(path, listing, error, cx)),
        Section::Truncated { value, limit } => Some(
            v_flex()
                .w_full()
                .gap_1()
                .child(render_filesystem_facts(path, listing, value, cx))
                .child(field("Read limit", &format_size(*limit), cx))
                .into_any_element(),
        ),
    }
}

fn render_filesystem_fallback(
    path: &std::path::Path,
    listing: &crate::properties::ListingFacts,
    error: &str,
    cx: &App,
) -> AnyElement {
    let mut section = v_flex()
        .w_full()
        .gap_1()
        .child(section_title("Filesystem", cx))
        .child(field("Name", &listing.name, cx))
        .child(field("Path", &path.display().to_string(), cx))
        .child(field(
            "Status",
            &format!("Native stat unavailable: {error}"),
            cx,
        ))
        .child(field(
            "Listed type",
            if listing.is_dir { "Directory" } else { "File" },
            cx,
        ));
    if let Some(size) = listing.size {
        section = section.child(field("Listed size", &format_size(size), cx));
    }
    if let Some(modified) = listing.modified {
        section = section.child(field(
            "Listed modified",
            &crate::fs_ops::format_time(modified),
            cx,
        ));
    }
    if let Some(attrs) = listing.attrs {
        section = section.child(field(
            "Listed permissions",
            &attrs.permissions(listing.is_dir),
            cx,
        ));
        if let Some(uid) = attrs.uid {
            section = section.child(field("Listed owner ID", &uid.to_string(), cx));
        }
        section = section.child(field(
            "Listed read only",
            if attrs.readonly { "Yes" } else { "No" },
            cx,
        ));
    }
    section.into_any_element()
}

fn render_filesystem_facts(
    path: &std::path::Path,
    listing: &crate::properties::ListingFacts,
    facts: &crate::properties::FilesystemFacts,
    cx: &App,
) -> AnyElement {
    let stat = &facts.link_object;
    let mut section = v_flex()
        .w_full()
        .gap_1()
        .child(section_title("Filesystem", cx))
        .child(field("Name", &listing.name, cx))
        .child(field("Path", &path.display().to_string(), cx))
        .child(field(
            "Object",
            object_type(
                facts.symlink_target.is_some() || facts.target_error.is_some(),
                stat.is_dir,
                stat.is_file,
            ),
            cx,
        ));
    if let Some(size) = stat.size {
        section = section.child(field("Size", &format_size(size), cx));
    }
    if let Some(time) = stat.modified {
        section = section.child(field("Modified", &crate::fs_ops::format_time(time), cx));
    }
    if let Some(time) = stat.created {
        section = section.child(field("Created", &crate::fs_ops::format_time(time), cx));
    }
    section = section.child(field(
        "Permissions",
        &stat.attrs.permissions(stat.is_dir),
        cx,
    ));
    if let Some(uid) = stat.attrs.uid {
        section = section.child(field("Owner ID", &uid.to_string(), cx));
    }
    if stat.attrs.readonly {
        section = section.child(field("Read only", "Yes", cx));
    } else {
        section = section.child(field("Read only", "No", cx));
    }
    if facts.symlink_target.is_some() || facts.target_error.is_some() {
        let mut link = v_flex()
            .w_full()
            .gap_1()
            .child(section_title("Symlink", cx));
        if let Some(target) = &facts.symlink_target {
            link = link.child(field("Target path", &target.display().to_string(), cx));
        }
        if let Some(target) = &facts.target {
            link = link.child(field(
                "Target type",
                object_type(false, target.is_dir, target.is_file),
                cx,
            ));
            if let Some(size) = target.size {
                link = link.child(field("Target size", &format_size(size), cx));
            }
            if let Some(modified) = target.modified {
                link = link.child(field(
                    "Target modified",
                    &crate::fs_ops::format_time(modified),
                    cx,
                ));
            }
            link = link.child(field(
                "Target permissions",
                &target.attrs.permissions(target.is_dir),
                cx,
            ));
            if let Some(uid) = target.attrs.uid {
                link = link.child(field("Target owner ID", &uid.to_string(), cx));
            }
            link = link.child(field(
                "Target read only",
                if target.attrs.readonly { "Yes" } else { "No" },
                cx,
            ));
        }
        if let Some(error) = &facts.target_error {
            link = link.child(field("Target status", &format!("Unavailable: {error}"), cx));
        }
        section = section.child(link);
    }
    section.into_any_element()
}

fn object_type(is_symlink: bool, is_dir: bool, is_file: bool) -> &'static str {
    if is_symlink {
        "Symlink"
    } else if is_dir {
        "Directory"
    } else if is_file {
        "File"
    } else {
        "Other"
    }
}

fn render_exif(value: &Section<Vec<crate::properties::ExifField>>, cx: &App) -> Option<AnyElement> {
    match value {
        Section::Available(fields) => {
            if fields.is_empty() {
                return None;
            }
            let mut section = v_flex().w_full().gap_1().child(section_title("EXIF", cx));
            for item in fields {
                section = section.child(field(exif_label(item.id), &item.value, cx));
            }
            Some(section.into_any_element())
        }
        Section::Unavailable(_) => None,
        Section::Failed(error) => {
            Some(status_section("EXIF", &format!("Unavailable: {error}"), cx))
        }
        Section::Truncated { value, limit } => {
            let mut section = v_flex().w_full().gap_1().child(section_title("EXIF", cx));
            for item in value {
                section = section.child(field(exif_label(item.id), &item.value, cx));
            }
            Some(
                section
                    .child(field("Read limit", &format_size(*limit), cx))
                    .into_any_element(),
            )
        }
    }
}

fn status_section(title: &str, message: &str, cx: &App) -> AnyElement {
    v_flex()
        .w_full()
        .gap_1()
        .child(section_title(title, cx))
        .child(field("Status", message, cx))
        .into_any_element()
}

fn exif_label(id: crate::properties::ExifFieldId) -> &'static str {
    use crate::properties::ExifFieldId::*;
    match id {
        Make => "Camera maker",
        Model => "Camera model",
        LensModel => "Lens model",
        DateTimeOriginal => "Date taken",
        ExposureTime => "Exposure time",
        FNumber => "Aperture",
        PhotographicSensitivity => "ISO",
        FocalLength => "Focal length",
        PixelXDimension => "Width",
        PixelYDimension => "Height",
        GPSLatitude => "GPS latitude",
        GPSLongitude => "GPS longitude",
        Software => "Software",
    }
}

fn humanize(value: &str) -> String {
    value
        .replace(['_', '-'], " ")
        .split_whitespace()
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn section_title(title: &str, cx: &App) -> impl IntoElement {
    div()
        .mt_2()
        .pb_1()
        .border_b_1()
        .border_color(cx.theme().border)
        .text_sm()
        .font_semibold()
        .child(title.to_owned())
}

fn field(label: &str, value: &str, cx: &App) -> impl IntoElement {
    h_flex()
        .w_full()
        .gap_2()
        .child(
            div()
                .w(px(120.))
                .flex_none()
                .text_color(cx.theme().muted_foreground)
                .child(label.to_owned()),
        )
        .child(div().flex_1().min_w_0().text_sm().child(value.to_owned()))
}

fn format_value(value: &PropertyValue) -> String {
    match value {
        PropertyValue::Text(value) => value.clone(),
        PropertyValue::Unsigned(value) => value.to_string(),
        PropertyValue::Signed(value) => value.to_string(),
        PropertyValue::Boolean(value) => value.to_string(),
        PropertyValue::Time(value) => crate::fs_ops::format_time(*value),
        PropertyValue::Path(value) => value.display().to_string(),
    }
}

fn format_size(size: u64) -> String {
    crate::fs_ops::format_size(size)
}
