//! Properties perspective: schedules bounded inspection and renders semantic groups.

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::*;

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
    body = body.child(section_title("Source", cx));
    body = body.child(field("Name", &listing.name, cx));
    body = body.child(field(
        "Type",
        if listing.is_dir { "Directory" } else { "File" },
        cx,
    ));
    if let Some(size) = listing.size {
        body = body.child(field("Size", &crate::fs_ops::format_size(size), cx));
    }
    if let Some(modified) = listing.modified {
        body = body.child(field("Modified", &crate::fs_ops::format_time(modified), cx));
    }
    if let Some(attrs) = listing.attrs {
        body = body.child(field("Permissions", &attrs.permissions(listing.is_dir), cx));
        if let Some(uid) = attrs.uid {
            body = body.child(field("Owner ID", &uid.to_string(), cx));
        }
        if attrs.readonly {
            body = body.child(field("Read only", "Yes", cx));
        }
    }
    if let Some(link) = listing.link.tooltip_with_resolved(None) {
        body = body.child(field("Link", &link, cx));
    }
    body = body.child(render_filesystem(&inspection.filesystem, cx));
    body = body.child(render_exif(&inspection.exif, cx));
    for group in &inspection.additional_groups {
        body = body.child(render_group(group, cx));
    }
    body.into_any_element()
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
        GroupStatus::Truncated { limit } => {
            content = content.child(field(
                "Status",
                &format!("Truncated at {}", format_size(*limit)),
                cx,
            ));
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
        content = content.child(render_group(nested, cx));
    }
    for table in &group.tables {
        content = content.child(section_title(&humanize(&table.id), cx));
        content = content.child(
            h_flex()
                .gap_2()
                .children(table.columns.iter().map(|column| {
                    div()
                        .flex_1()
                        .text_xs()
                        .font_semibold()
                        .child(humanize(column))
                })),
        );
        for row in &table.rows {
            content = content.child(h_flex().gap_2().children(row.iter().map(|value| {
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .child(format_value(value))
            })));
        }
    }
    content.into_any_element()
}

fn render_filesystem(value: &Section<crate::properties::FilesystemFacts>, cx: &App) -> AnyElement {
    match value {
        Section::Available(value) => render_filesystem_facts(value, cx),
        Section::Unavailable(reason) => status_section("Filesystem", reason, cx),
        Section::Failed(error) => {
            status_section("Filesystem", &format!("Unavailable: {error}"), cx)
        }
        Section::Truncated { value, limit } => v_flex()
            .w_full()
            .gap_1()
            .child(render_filesystem_facts(value, cx))
            .child(field("Read limit", &format_size(*limit), cx))
            .into_any_element(),
    }
}

fn render_filesystem_facts(facts: &crate::properties::FilesystemFacts, cx: &App) -> AnyElement {
    let stat = &facts.link_object;
    let mut section = v_flex()
        .w_full()
        .gap_1()
        .child(section_title("Filesystem", cx));
    section = section.child(field(
        "Object",
        if stat.is_dir {
            "Directory"
        } else if stat.is_file {
            "File"
        } else {
            "Other"
        },
        cx,
    ));
    if let Some(size) = stat.size {
        section = section.child(field("Local size", &format_size(size), cx));
    }
    if let Some(time) = stat.modified {
        section = section.child(field(
            "Local modified",
            &crate::fs_ops::format_time(time),
            cx,
        ));
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
    }
    if let Some(target) = &facts.symlink_target {
        section = section.child(field("Link target", &target.display().to_string(), cx));
    }
    if let Some(target) = &facts.target {
        section = section.child(field(
            "Target",
            if target.is_dir {
                "Directory"
            } else if target.is_file {
                "File"
            } else {
                "Other"
            },
            cx,
        ));
    }
    if let Some(error) = &facts.target_error {
        section = section.child(field("Target status", &format!("Unavailable: {error}"), cx));
    }
    section.into_any_element()
}

fn render_exif(value: &Section<Vec<crate::properties::ExifField>>, cx: &App) -> AnyElement {
    match value {
        Section::Available(fields) => {
            let mut section = v_flex().w_full().gap_1().child(section_title("EXIF", cx));
            if fields.is_empty() {
                section = section.child(field("Metadata", "No EXIF fields", cx));
            }
            for item in fields {
                section = section.child(field(exif_label(item.id), &item.value, cx));
            }
            section.into_any_element()
        }
        Section::Unavailable(reason) => status_section("EXIF", reason, cx),
        Section::Failed(error) => status_section("EXIF", &format!("Unavailable: {error}"), cx),
        Section::Truncated { value, limit } => {
            let mut section = v_flex().w_full().gap_1().child(section_title("EXIF", cx));
            for item in value {
                section = section.child(field(exif_label(item.id), &item.value, cx));
            }
            section
                .child(field("Read limit", &format_size(*limit), cx))
                .into_any_element()
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
