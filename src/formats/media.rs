//! Bounded, timed `ffprobe` JSON reading for local media metadata.

use std::io::{self, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

/// Maximum media streams retained for presentation.
pub const STREAM_LIMIT: usize = 32;
const OUTPUT_LIMIT: usize = 1024 * 1024;
const ERROR_LIMIT: usize = 8192;
const TEXT_LIMIT: usize = 1024;
const TAG_LIMIT: usize = 16;
const DEADLINE: Duration = Duration::from_secs(10);

/// One selected media stream, excluding attached cover art from video detection.
#[derive(Clone, Debug, PartialEq)]
pub struct Stream {
    /// Video, audio, or subtitle.
    pub kind: StreamKind,
    /// Codec short name, when supplied.
    pub codec: Option<String>,
    /// Video width in pixels.
    pub width: Option<u64>,
    /// Video height in pixels.
    pub height: Option<u64>,
    /// Video frame rate in frames per second.
    pub frame_rate: Option<f64>,
    /// Audio sample rate in hertz.
    pub sample_rate: Option<u64>,
    /// Audio channel count.
    pub channels: Option<u64>,
    /// Language tag, when supplied.
    pub language: Option<String>,
}

/// Stream category for display and video-frame selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamKind {
    /// Video image stream.
    Video,
    /// Audio stream.
    Audio,
    /// Subtitle stream.
    Subtitle,
}

/// Typed, bounded facts from a completed media probe.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MediaMetadata {
    /// Container's descriptive format name.
    pub format: Option<String>,
    /// Finite, nonnegative duration in seconds.
    pub duration: Option<f64>,
    /// Container bit rate in bits per second.
    pub bit_rate: Option<u64>,
    /// Selected container tags with stable display names.
    pub tags: Vec<(String, String)>,
    /// Selected media streams.
    pub streams: Vec<Stream>,
    /// True when any non-cover-art video stream exists, including beyond the display cap.
    pub has_video_stream: bool,
    /// True when stream or text bounds omitted data.
    pub truncated: bool,
}

impl MediaMetadata {
    /// Whether a non-cover-art video stream was reported.
    pub fn has_video(&self) -> bool {
        self.has_video_stream
    }
}

/// Reads a regular local file with a bounded, cancellable `ffprobe` process.
pub fn probe(path: &Path, cancelled: impl Fn() -> bool) -> io::Result<MediaMetadata> {
    probe_with("ffprobe", path, cancelled)
}

fn probe_with(
    program: &str,
    path: &Path,
    cancelled: impl Fn() -> bool,
) -> io::Result<MediaMetadata> {
    if !std::fs::metadata(path)?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    if cancelled() {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "media probe cancelled",
        ));
    }
    let mut child = Command::new(program)
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
            "-probesize",
            "10000000",
            "-analyzeduration",
            "5000000",
            "-protocol_whitelist",
            "file",
            "-i",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let overflow = Arc::new(AtomicBool::new(false));
    let stdout = child.stdout.take().expect("piped stdout exists");
    let stderr = child.stderr.take().expect("piped stderr exists");
    let stdout_overflow = overflow.clone();
    let stderr_overflow = overflow.clone();
    let output_reader = thread::spawn(move || read_bounded(stdout, OUTPUT_LIMIT, stdout_overflow));
    let error_reader = thread::spawn(move || read_bounded(stderr, ERROR_LIMIT, stderr_overflow));
    let start = Instant::now();
    let outcome = loop {
        if cancelled() {
            break Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "media probe cancelled",
            ));
        }
        if start.elapsed() >= DEADLINE {
            break Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "media probe timed out",
            ));
        }
        if overflow.load(Ordering::Relaxed) {
            break Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "ffprobe output exceeds limit",
            ));
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {}
            Err(error) => break Err(error),
        }
        thread::sleep(Duration::from_millis(20));
    };
    if outcome.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let output = output_reader
        .join()
        .map_err(|_| io::Error::other("ffprobe output reader panicked"))??;
    let error = error_reader
        .join()
        .map_err(|_| io::Error::other("ffprobe error reader panicked"))??;
    let status = outcome?;
    if overflow.load(Ordering::Relaxed) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "ffprobe output exceeds limit",
        ));
    }
    if !status.success() {
        let detail = String::from_utf8_lossy(&error).trim().to_owned();
        return Err(io::Error::other(if detail.is_empty() {
            "ffprobe failed".into()
        } else {
            detail
        }));
    }
    parse(&output)
}

fn read_bounded(
    mut reader: impl Read,
    limit: usize,
    overflow: Arc<AtomicBool>,
) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        let remaining = limit.saturating_sub(bytes.len());
        bytes.extend_from_slice(&buffer[..count.min(remaining)]);
        if count > remaining {
            overflow.store(true, Ordering::Relaxed);
        }
    }
    Ok(bytes)
}

/// Parses bounded `ffprobe` JSON into typed media facts.
pub fn parse(json: &[u8]) -> io::Result<MediaMetadata> {
    if json.len() > OUTPUT_LIMIT {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "ffprobe output exceeds limit",
        ));
    }
    let root: Value = serde_json::from_slice(json).map_err(io::Error::other)?;
    let format = &root["format"];
    let mut result = MediaMetadata {
        duration: finite_nonnegative(&format["duration"]),
        bit_rate: integer(&format["bit_rate"]),
        ..MediaMetadata::default()
    };
    result.format = bounded_text(format["format_long_name"].as_str(), &mut result.truncated);
    const TAGS: &[(&str, &str)] = &[
        ("title", "Title"),
        ("artist", "Artist"),
        ("album", "Album"),
        ("album_artist", "Album artist"),
        ("composer", "Composer"),
        ("date", "Date"),
        ("genre", "Genre"),
        ("track", "Track"),
        ("disc", "Disc"),
        ("comment", "Comment"),
        ("copyright", "Copyright"),
    ];
    if let Some(tags) = format["tags"].as_object() {
        for (key, label) in TAGS.iter().take(TAG_LIMIT) {
            let value = tags
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(key))
                .and_then(|(_, value)| value.as_str());
            if let Some(value) = bounded_text(value, &mut result.truncated) {
                result.tags.push(((*label).into(), value));
            }
        }
    }
    if let Some(streams) = root["streams"].as_array() {
        for stream in streams {
            let kind = match stream["codec_type"].as_str() {
                Some("video") if stream["disposition"]["attached_pic"].as_i64() != Some(1) => {
                    StreamKind::Video
                }
                Some("audio") => StreamKind::Audio,
                Some("subtitle") => StreamKind::Subtitle,
                _ => continue,
            };
            if kind == StreamKind::Video {
                result.has_video_stream = true;
            }
            if result.streams.len() == STREAM_LIMIT {
                result.truncated = true;
                break;
            }
            result.streams.push(Stream {
                kind,
                codec: bounded_text(stream["codec_name"].as_str(), &mut result.truncated),
                width: stream["width"].as_u64(),
                height: stream["height"].as_u64(),
                frame_rate: stream["avg_frame_rate"].as_str().and_then(frame_rate),
                sample_rate: integer(&stream["sample_rate"]),
                channels: stream["channels"].as_u64(),
                language: bounded_text(stream["tags"]["language"].as_str(), &mut result.truncated),
            });
        }
    }
    Ok(result)
}

fn bounded_text(value: Option<&str>, truncated: &mut bool) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    let mut chars = value.chars();
    let selected: String = chars.by_ref().take(TEXT_LIMIT).collect();
    if chars.next().is_some() {
        *truncated = true;
    }
    Some(selected)
}

fn finite_nonnegative(value: &Value) -> Option<f64> {
    let number = value
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .or_else(|| value.as_f64())?;
    (number.is_finite() && number >= 0.0).then_some(number)
}

fn integer(value: &Value) -> Option<u64> {
    value
        .as_str()
        .and_then(|s| s.parse().ok())
        .or_else(|| value.as_u64())
}

fn frame_rate(value: &str) -> Option<f64> {
    let (numerator, denominator) = value.split_once('/')?;
    let numerator: f64 = numerator.parse().ok()?;
    let denominator: f64 = denominator.parse().ok()?;
    let rate = numerator / denominator;
    (rate.is_finite() && rate > 0.0).then_some(rate)
}

/// Formats seconds as `mm:ss` or `h:mm:ss` with rounded whole seconds.
pub fn format_duration(seconds: f64) -> Option<String> {
    if !seconds.is_finite() || seconds < 0.0 || seconds > u64::MAX as f64 {
        return None;
    }
    let total = seconds.round() as u64;
    let (hours, minutes, seconds) = (total / 3600, total / 60 % 60, total % 60);
    Some(if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    })
}

#[cfg(test)]
mod tests {
    use super::{StreamKind, format_duration, parse};

    #[test]
    fn audio_tags_are_case_insensitive_bounded_and_allowlisted() {
        let input = serde_json::json!({"format": {"tags": {
            "TITLE": "Song", "album_artist": "Ensemble", "composer": "Composer",
            "disc": "2/3", "comment": "x".repeat(1100), "copyright": "Owner",
            "unknown": "Hidden", "track": 4
        }}});
        let metadata =
            parse(&serde_json::to_vec(&input).expect("JSON fixture")).expect("tag metadata");
        assert!(metadata.truncated);
        assert_eq!(
            metadata.tags,
            vec![
                ("Title".into(), "Song".into()),
                ("Album artist".into(), "Ensemble".into()),
                ("Composer".into(), "Composer".into()),
                ("Disc".into(), "2/3".into()),
                ("Comment".into(), "x".repeat(1024)),
                ("Copyright".into(), "Owner".into()),
            ]
        );
    }

    #[test]
    fn rejects_bad_durations_and_bounds_streams() {
        let metadata = parse(br#"{"format":{"duration":"NaN"},"streams":[{"codec_type":"video","disposition":{"attached_pic":1}},{"codec_type":"audio","codec_name":"aac"}]}"#).unwrap();
        assert_eq!(metadata.duration, None);
        assert_eq!(metadata.streams.len(), 1);
        assert_eq!(metadata.streams[0].kind, StreamKind::Audio);
        assert!(!metadata.has_video());
        assert_eq!(format_duration(3725.4).as_deref(), Some("1:02:05"));
        assert_eq!(format_duration(-1.0), None);
    }

    #[test]
    fn video_detection_survives_stream_display_cap() {
        let mut streams = vec![serde_json::json!({"codec_type": "audio"}); 32];
        streams.push(serde_json::json!({"codec_type": "video"}));
        let input = serde_json::json!({"streams": streams});
        let metadata = parse(&serde_json::to_vec(&input).expect("JSON fixture")).expect("probe");
        assert!(metadata.has_video());
        assert_eq!(metadata.streams.len(), 32);
        assert!(metadata.truncated);
    }

    #[cfg(unix)]
    #[test]
    fn fake_probe_bounds_output_and_reports_failures() {
        use std::io::ErrorKind;
        use std::os::unix::fs::PermissionsExt;

        use tempfile::TempDir;

        use super::probe_with;

        let temp = TempDir::new().expect("temp directory");
        let media = temp.path().join("clip.mp4");
        std::fs::write(&media, b"fixture").expect("media fixture");
        let program = temp.path().join("fake-probe");
        let install = |script: &str| {
            std::fs::write(&program, format!("#!/bin/sh\n{script}\n")).expect("fake probe");
            let mut mode = std::fs::metadata(&program)
                .expect("fake probe metadata")
                .permissions();
            mode.set_mode(0o755);
            std::fs::set_permissions(&program, mode).expect("fake probe mode");
        };
        install("printf '%s' '{\"format\":{\"duration\":\"2.0\"}}'");
        assert_eq!(
            probe_with(program.to_str().expect("UTF-8 temp path"), &media, || false)
                .expect("valid probe")
                .duration,
            Some(2.0)
        );
        install("printf 'bad media' >&2; exit 1");
        assert!(
            probe_with(program.to_str().expect("UTF-8 temp path"), &media, || false)
                .expect_err("failed probe")
                .to_string()
                .contains("bad media")
        );
        install("i=0; while [ \"$i\" -lt 1100 ]; do printf '%01024d' 0; i=$((i+1)); done");
        assert_eq!(
            probe_with(program.to_str().expect("UTF-8 temp path"), &media, || false)
                .expect_err("oversized probe")
                .kind(),
            ErrorKind::InvalidData
        );
        install("while :; do :; done");
        let cancelled = std::sync::atomic::AtomicBool::new(false);
        assert_eq!(
            probe_with(program.to_str().expect("UTF-8 temp path"), &media, || {
                cancelled.swap(true, std::sync::atomic::Ordering::Relaxed)
            })
            .expect_err("cancelled probe")
            .kind(),
            ErrorKind::Interrupted
        );
    }
}
