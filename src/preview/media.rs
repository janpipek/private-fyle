//! Audio and video: format, streams and tags from `ffprobe`; for video a
//! frame grabbed by `ffmpeg`. Without ffmpeg, a note saying what to install.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use super::{Content, Previewer, Probe};
use crate::icons::{self, FileIcon};

pub struct MediaPreviewer;

impl Previewer for MediaPreviewer {
    fn name(&self) -> &'static str {
        "Media"
    }

    fn matches(&self, probe: &Probe) -> bool {
        !probe.is_dir
            && matches!(
                icons::icon_for(&probe.name, false, false, false),
                FileIcon::Audio | FileIcon::Video
            )
    }

    fn load(&self, probe: &Probe) -> io::Result<Content> {
        let output = match Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-print_format",
                "json",
                "-show_format",
                "-show_streams",
            ])
            .arg(&probe.path)
            .output()
        {
            Ok(output) => output,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Ok(Content::Text {
                    text: "Audio and video details need ffprobe from ffmpeg \
                           (e.g. dnf install ffmpeg-free, apt install ffmpeg)."
                        .into(),
                    language: None,
                    truncated: false,
                });
            }
            Err(e) => return Err(e),
        };
        if !output.status.success() {
            let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(io::Error::other(if message.is_empty() {
                "ffprobe failed".into()
            } else {
                message
            }));
        }
        let probed = Probed::parse(&output.stdout)?;
        let info = probed.info();
        if probed.has_video {
            // A frame a tenth of the way in: past black intros, still early.
            if let Ok(frame) = grab_frame(&probe.path, probed.duration.unwrap_or(0.) / 10.) {
                return Ok(Content::Image { path: frame, info });
            }
        }
        Ok(Content::Info { info })
    }
}

/// What `ffprobe` reported.
#[derive(Debug, Default, PartialEq)]
struct Probed {
    duration: Option<f64>,
    has_video: bool,
    format: Option<String>,
    bit_rate: Option<u64>,
    /// Stream descriptions, e.g. "h264 1920×1080 25 fps".
    streams: Vec<String>,
    /// Container tags worth showing (title, artist, …), as labelled.
    tags: Vec<(String, String)>,
}

impl Probed {
    fn parse(json: &[u8]) -> io::Result<Self> {
        let root: Value = serde_json::from_slice(json).map_err(io::Error::other)?;
        let format = &root["format"];
        let number = |v: &Value| v.as_str().and_then(|s| s.parse::<f64>().ok());
        let mut probed = Self {
            duration: number(&format["duration"]),
            format: format["format_long_name"].as_str().map(String::from),
            bit_rate: number(&format["bit_rate"]).map(|b| b as u64),
            ..Self::default()
        };
        for stream in root["streams"].as_array().into_iter().flatten() {
            let codec = stream["codec_name"].as_str().unwrap_or("?");
            // Cover art in audio files is a "video" stream with this flag.
            let attached = stream["disposition"]["attached_pic"].as_i64() == Some(1);
            match stream["codec_type"].as_str() {
                Some("video") if !attached => {
                    probed.has_video = true;
                    let mut text = codec.to_string();
                    if let (Some(w), Some(h)) =
                        (stream["width"].as_u64(), stream["height"].as_u64())
                    {
                        text += &format!(" {w}×{h}");
                    }
                    if let Some(fps) = stream["avg_frame_rate"].as_str().and_then(frame_rate) {
                        text += &format!(" {fps} fps");
                    }
                    probed.streams.push(format!("Video: {text}"));
                }
                Some("audio") => {
                    let mut text = codec.to_string();
                    if let Some(rate) = number(&stream["sample_rate"]) {
                        text += &format!(" {} kHz", rate / 1000.);
                    }
                    match stream["channels"].as_u64() {
                        Some(1) => text += " mono",
                        Some(2) => text += " stereo",
                        Some(n) => text += &format!(" {n} channels"),
                        None => {}
                    }
                    probed.streams.push(format!("Audio: {text}"));
                }
                Some("subtitle") => {
                    let language = stream["tags"]["language"].as_str().unwrap_or("");
                    probed
                        .streams
                        .push(format!("Subtitles: {codec} {language}").trim().into());
                }
                _ => {}
            }
        }
        const TAGS: &[(&str, &str)] = &[
            ("title", "Title"),
            ("artist", "Artist"),
            ("album", "Album"),
            ("date", "Date"),
            ("genre", "Genre"),
            ("track", "Track"),
        ];
        if let Some(tags) = format["tags"].as_object() {
            for (key, label) in TAGS {
                // Tag names vary in case between containers.
                let value = tags
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(key))
                    .and_then(|(_, v)| v.as_str());
                if let Some(value) = value.filter(|v| !v.trim().is_empty()) {
                    probed
                        .tags
                        .push((label.to_string(), value.trim().to_string()));
                }
            }
        }
        Ok(probed)
    }

    fn info(&self) -> Vec<(String, String)> {
        let mut info = self.tags.clone();
        if let Some(d) = self.duration {
            info.push(("Duration".into(), format_duration(d)));
        }
        for stream in &self.streams {
            if let Some((kind, text)) = stream.split_once(": ") {
                info.push((kind.into(), text.into()));
            }
        }
        if let Some(format) = &self.format {
            info.push(("Format".into(), format.clone()));
        }
        if let Some(rate) = self.bit_rate {
            info.push(("Bit rate".into(), format!("{} kb/s", rate / 1000)));
        }
        info
    }
}

/// `"30000/1001"` → `"29.97"`; `None` for `"0/0"`.
fn frame_rate(ratio: &str) -> Option<String> {
    let (n, d) = ratio.split_once('/')?;
    let (n, d): (f64, f64) = (n.parse().ok()?, d.parse().ok()?);
    if n == 0. || d == 0. {
        return None;
    }
    let fps = n / d;
    Some(if fps.fract().abs() < 0.005 {
        format!("{fps:.0}")
    } else {
        format!("{fps:.2}")
    })
}

/// `3725.4` → `"1:02:05"`, `65.0` → `"1:05"`.
fn format_duration(seconds: f64) -> String {
    let total = seconds.round() as u64;
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// One frame at `seconds`, scaled down, cached as PNG.
fn grab_frame(video: &Path, seconds: f64) -> io::Result<PathBuf> {
    let png = super::cache_file(video, "png")?;
    if png.exists() {
        return Ok(png);
    }
    let output = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-ss", &format!("{seconds:.2}"), "-i"])
        .arg(video)
        .args(["-frames:v", "1", "-vf", "scale='min(1200,iw)':-2"])
        .arg(&png)
        .output()?;
    if !output.status.success() || !png.exists() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    Ok(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIDEO: &str = r#"{
      "streams": [
        {"codec_type": "video", "codec_name": "h264", "width": 1920, "height": 1080,
         "avg_frame_rate": "30000/1001", "disposition": {"attached_pic": 0}},
        {"codec_type": "audio", "codec_name": "aac", "sample_rate": "48000", "channels": 2},
        {"codec_type": "subtitle", "codec_name": "subrip", "tags": {"language": "eng"}}
      ],
      "format": {"format_long_name": "QuickTime / MOV", "duration": "3725.4",
                 "bit_rate": "5000000", "tags": {"TITLE": "Trip", "encoder": "x"}}
    }"#;

    const AUDIO: &str = r#"{
      "streams": [
        {"codec_type": "audio", "codec_name": "flac", "sample_rate": "44100", "channels": 1},
        {"codec_type": "video", "codec_name": "mjpeg", "width": 500, "height": 500,
         "disposition": {"attached_pic": 1}}
      ],
      "format": {"duration": "65.0", "tags": {"artist": "Band", "title": " ", "album": "LP"}}
    }"#;

    fn info(json: &str) -> Vec<(String, String)> {
        Probed::parse(json.as_bytes()).unwrap().info()
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn describes_video() {
        assert!(Probed::parse(VIDEO.as_bytes()).unwrap().has_video);
        assert_eq!(
            info(VIDEO),
            pairs(&[
                ("Title", "Trip"),
                ("Duration", "1:02:05"),
                ("Video", "h264 1920×1080 29.97 fps"),
                ("Audio", "aac 48 kHz stereo"),
                ("Subtitles", "subrip eng"),
                ("Format", "QuickTime / MOV"),
                ("Bit rate", "5000 kb/s"),
            ])
        );
    }

    #[test]
    fn describes_audio_ignoring_cover_art() {
        assert!(!Probed::parse(AUDIO.as_bytes()).unwrap().has_video);
        assert_eq!(
            info(AUDIO),
            pairs(&[
                ("Artist", "Band"),
                ("Album", "LP"),
                ("Duration", "1:05"),
                ("Audio", "flac 44.1 kHz mono"),
            ])
        );
    }

    #[test]
    fn helpers() {
        assert_eq!(frame_rate("25/1").as_deref(), Some("25"));
        assert_eq!(frame_rate("0/0"), None);
        assert_eq!(format_duration(59.6), "1:00");
        assert!(Probed::parse(b"not json").is_err());
    }
}
