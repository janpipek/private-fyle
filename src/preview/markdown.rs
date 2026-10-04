use std::fs::File;
use std::io::{self, Read};

use super::{Content, Previewer, Probe};

const MAX_MARKDOWN: u64 = 512 * 1024;

/// Markdown rendered as formatted text (F4 shows the source).
pub struct MarkdownPreviewer;

impl Previewer for MarkdownPreviewer {
    fn name(&self) -> &'static str {
        "Markdown"
    }

    fn matches(&self, probe: &Probe) -> bool {
        !probe.is_dir && matches!(probe.ext.as_str(), "md" | "markdown") && probe.looks_like_text()
    }

    fn load(&self, probe: &Probe) -> io::Result<Content> {
        let mut bytes = Vec::new();
        File::open(&probe.path)?
            .take(MAX_MARKDOWN)
            .read_to_end(&mut bytes)?;
        Ok(Content::Markdown {
            text: String::from_utf8_lossy(&bytes).into_owned(),
        })
    }
}
