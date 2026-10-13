//! Audio and video details from the shared bounded media reader; video frames from ffmpeg.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{Content, Previewer, Probe};
use crate::formats::media::{self, MediaMetadata, StreamKind};
use crate::icons::{self, FileIcon};

/// Preview adapter for local audio and video files.
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
        // Preview currently has no cancellation signal. The shared reader still
        // enforces its deadline and output bounds for this call.
        let probed = match media::probe(&probe.path, || false) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound && probe.path.is_file() => {
                return Ok(Content::Text {
                    text: "Audio and video details need ffprobe from ffmpeg \
                           (e.g. dnf install ffmpeg-free, apt install ffmpeg)."
                        .into(),
                    language: None,
                    truncated: false,
                });
            }
            Err(error) => return Err(error),
        };
        if probed.has_video() {
            // A frame a tenth of the way in: past black intros, still early.
            let frame = grab_frame(&probe.path, probed.duration.unwrap_or(0.) / 10.)?;
            return Ok(Content::Image { path: frame });
        }
        Ok(Content::Info {
            info: info(&probed),
        })
    }
}

fn info(media: &MediaMetadata) -> Vec<(String, String)> {
    let mut info = media.tags.clone();
    if let Some(duration) = media.duration.and_then(media::format_duration) {
        info.push((
            "Duration".into(),
            duration.strip_prefix('0').unwrap_or(&duration).into(),
        ));
    }
    for stream in &media.streams {
        let codec = stream.codec.as_deref().unwrap_or("?");
        let (label, mut text) = match stream.kind {
            StreamKind::Video => ("Video", codec.to_string()),
            StreamKind::Audio => ("Audio", codec.to_string()),
            StreamKind::Subtitle => ("Subtitles", codec.to_string()),
        };
        match stream.kind {
            StreamKind::Video => {
                if let (Some(width), Some(height)) = (stream.width, stream.height) {
                    text += &format!(" {width}×{height}");
                }
                if let Some(fps) = stream.frame_rate {
                    let fps = if fps.fract().abs() < 0.005 {
                        format!("{fps:.0}")
                    } else {
                        format!("{fps:.2}")
                    };
                    text += &format!(" {fps} fps");
                }
            }
            StreamKind::Audio => {
                if let Some(rate) = stream.sample_rate {
                    text += &format!(" {} kHz", rate as f64 / 1000.0);
                }
                match stream.channels {
                    Some(1) => text += " mono",
                    Some(2) => text += " stereo",
                    Some(channels) => text += &format!(" {channels} channels"),
                    None => {}
                }
            }
            StreamKind::Subtitle => {
                if let Some(language) = &stream.language {
                    text += &format!(" {language}");
                }
            }
        }
        info.push((label.into(), text));
    }
    if let Some(format) = &media.format {
        info.push(("Format".into(), format.clone()));
    }
    if let Some(rate) = media.bit_rate {
        info.push(("Bit rate".into(), format!("{} kb/s", rate / 1000)));
    }
    if media.truncated {
        info.push((
            "Note".into(),
            "Media details truncated at safety limit".into(),
        ));
    }
    info
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
    use super::info;
    use crate::formats::media::{format_duration, parse};

    #[test]
    fn describes_video_and_audio_ignoring_cover_art() {
        let video = parse(br#"{"streams":[{"codec_type":"video","codec_name":"h264","width":1920,"height":1080,"avg_frame_rate":"30000/1001"},{"codec_type":"audio","codec_name":"aac","sample_rate":"48000","channels":2}],"format":{"format_long_name":"QuickTime / MOV","duration":"3725.4","bit_rate":"5000000","tags":{"TITLE":"Trip"}}}"#).unwrap();
        assert!(video.has_video());
        assert!(info(&video).contains(&("Video".into(), "h264 1920×1080 29.97 fps".into())));
        assert!(info(&video).contains(&("Duration".into(), "1:02:05".into())));
        let audio = parse(br#"{"streams":[{"codec_type":"audio","codec_name":"flac","sample_rate":"44100","channels":1},{"codec_type":"video","codec_name":"mjpeg","disposition":{"attached_pic":1}}],"format":{"duration":"65.0"}}"#).unwrap();
        assert!(!audio.has_video());
        assert!(info(&audio).contains(&("Audio".into(), "flac 44.1 kHz mono".into())));
        assert!(info(&audio).contains(&("Duration".into(), "1:05".into())));
        assert_eq!(format_duration(59.6).as_deref(), Some("01:00"));
    }
}
