//! Media stream facts mapped into independent Properties groups.

use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::formats::media::{self, MediaMetadata, Stream, StreamKind};
use crate::icons::{self, FileIcon};
use crate::properties::{
    GroupId, GroupStatus, Probe, PropertyField, PropertyGroup, PropertyProvider, PropertyValue,
};

/// Media metadata provider for the Properties registry.
pub(super) struct Provider;

impl PropertyProvider for Provider {
    fn id(&self) -> &'static str {
        "container"
    }

    fn matches(&self, probe: &Probe) -> bool {
        probe.is_file && is_candidate(&probe.path)
    }

    fn read(&self, probe: &Probe, cancel: &Arc<AtomicBool>) -> io::Result<Vec<PropertyGroup>> {
        read(&probe.path, cancel)
    }

    fn failure(&self, error: &io::Error) -> PropertyGroup {
        failure(error)
    }
}

/// Whether a local regular path is a media file supported by the media preview.
pub fn is_candidate(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    matches!(
        icons::icon_for(name, false, false, false),
        FileIcon::Audio | FileIcon::Video
    )
}

/// Reads local media metadata and maps it to bounded semantic groups.
pub fn read(path: &Path, cancel: &AtomicBool) -> io::Result<Vec<PropertyGroup>> {
    let data = media::probe(path, || cancel.load(Ordering::Relaxed))?;
    Ok(groups(&data))
}

/// A visible failure group that leaves unrelated Properties facts intact.
pub fn failure(error: &io::Error) -> PropertyGroup {
    let message = if error.kind() == io::ErrorKind::NotFound {
        "ffprobe is required for media details".into()
    } else {
        error.to_string()
    };
    PropertyGroup {
        id: GroupId("container".into()),
        status: GroupStatus::Failed(message),
        fields: Vec::new(),
        groups: Vec::new(),
        tables: Vec::new(),
    }
}

fn groups(data: &MediaMetadata) -> Vec<PropertyGroup> {
    let mut container = group("container");
    if let Some(format) = &data.format {
        field(&mut container, "format", format.clone());
    }
    if let Some(duration) = data.duration.and_then(media::format_duration) {
        field(&mut container, "duration", duration);
    }
    if let Some(rate) = data.bit_rate {
        container.fields.push(PropertyField {
            id: "bit_rate_bps".into(),
            value: PropertyValue::Unsigned(rate),
        });
    }
    for (label, value) in &data.tags {
        field(&mut container, &label.to_ascii_lowercase(), value.clone());
    }
    if data.truncated {
        container.status = GroupStatus::Truncated {
            reason: "Media details limited to 32 streams, 1,024 characters per value, and bounded process output".into(),
        };
    }
    let mut result = vec![container];
    for (kind, id) in [
        (StreamKind::Video, "video"),
        (StreamKind::Audio, "audio"),
        (StreamKind::Subtitle, "subtitles"),
    ] {
        let selected: Vec<_> = data
            .streams
            .iter()
            .filter(|stream| stream.kind == kind)
            .collect();
        if selected.is_empty() {
            continue;
        }
        let mut section = group(id);
        if selected.len() == 1 {
            append_stream(&mut section, selected[0]);
        } else {
            for (index, stream) in selected.into_iter().enumerate() {
                let mut nested = group(&format!("stream_{}", index + 1));
                append_stream(&mut nested, stream);
                section.groups.push(nested);
            }
        }
        result.push(section);
    }
    result
        .into_iter()
        .filter(PropertyGroup::is_visible)
        .collect()
}

fn group(id: &str) -> PropertyGroup {
    PropertyGroup {
        id: GroupId(id.into()),
        status: GroupStatus::Available,
        fields: Vec::new(),
        groups: Vec::new(),
        tables: Vec::new(),
    }
}

fn field(group: &mut PropertyGroup, id: &str, value: String) {
    group.fields.push(PropertyField {
        id: id.into(),
        value: PropertyValue::Text(value),
    });
}

fn append_stream(group: &mut PropertyGroup, stream: &Stream) {
    if let Some(codec) = &stream.codec {
        field(group, "codec", codec.clone());
    }
    if let Some(width) = stream.width {
        group.fields.push(PropertyField {
            id: "width_px".into(),
            value: PropertyValue::Unsigned(width),
        });
    }
    if let Some(height) = stream.height {
        group.fields.push(PropertyField {
            id: "height_px".into(),
            value: PropertyValue::Unsigned(height),
        });
    }
    if let Some(rate) = stream.frame_rate {
        field(group, "frame_rate_fps", format!("{rate:.2}"));
    }
    if let Some(rate) = stream.sample_rate {
        group.fields.push(PropertyField {
            id: "sample_rate_hz".into(),
            value: PropertyValue::Unsigned(rate),
        });
    }
    if let Some(channels) = stream.channels {
        group.fields.push(PropertyField {
            id: "channels".into(),
            value: PropertyValue::Unsigned(channels),
        });
    }
    if let Some(language) = &stream.language {
        field(group, "language", language.clone());
    }
}
