use std::fs::File;
use std::io::{self, Read};

use super::{Content, Previewer, Probe};

/// Larger files are cut off; the editor handles about 50K lines comfortably.
const MAX_TEXT: u64 = 512 * 1024;

pub struct TextPreviewer;

impl Previewer for TextPreviewer {
    fn name(&self) -> &'static str {
        "Text"
    }

    fn matches(&self, probe: &Probe) -> bool {
        !probe.is_dir && probe.looks_like_text()
    }

    fn load(&self, probe: &Probe) -> io::Result<Content> {
        let mut bytes = Vec::new();
        File::open(&probe.path)?
            .take(MAX_TEXT)
            .read_to_end(&mut bytes)?;
        Ok(Content::Text {
            text: String::from_utf8_lossy(&bytes).into_owned(),
            language: language_for(&probe.name, &probe.ext),
            truncated: probe.size > MAX_TEXT,
        })
    }
}

/// Highlighting language (as named by gpui-component) for a file.
pub fn language_for(name: &str, ext: &str) -> Option<&'static str> {
    let by_name = match name {
        "Makefile" | "makefile" | "GNUmakefile" => Some("make"),
        "CMakeLists.txt" => Some("cmake"),
        _ => None,
    };
    by_name.or(match ext {
        "rs" => Some("rust"),
        "py" | "pyi" => Some("python"),
        "js" | "mjs" | "cjs" => Some("javascript"),
        "ts" | "mts" | "cts" => Some("typescript"),
        "tsx" => Some("tsx"),
        "json" | "jsonc" => Some("json"),
        "toml" => Some("toml"),
        "yaml" | "yml" => Some("yaml"),
        "md" | "markdown" => Some("markdown"),
        "sh" | "bash" | "zsh" => Some("bash"),
        "c" | "h" => Some("c"),
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => Some("cpp"),
        "cs" => Some("csharp"),
        "go" => Some("go"),
        "java" => Some("java"),
        "kt" | "kts" => Some("kotlin"),
        "html" | "htm" => Some("html"),
        "css" | "scss" => Some("css"),
        "sql" => Some("sql"),
        "rb" => Some("ruby"),
        "lua" => Some("lua"),
        "php" => Some("php"),
        "swift" => Some("swift"),
        "scala" => Some("scala"),
        "zig" => Some("zig"),
        "diff" | "patch" => Some("diff"),
        "proto" => Some("proto"),
        "graphql" | "gql" => Some("graphql"),
        "ex" | "exs" => Some("elixir"),
        "cmake" => Some("cmake"),
        _ => None,
    })
}
