//! Tools implemented in Rust, for anything a command template cannot express.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::Command;

use super::{
    Action, CommandSpec, Facet, FacetKind, Facts, Invocation, Job, Output, Param, ParamKind,
    PickItem, Requirement, Target, Tool, ToolContext,
};
use crate::icons::FileIcon;

pub fn all() -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(OpenRepoWebsite),
        Box::new(JustTasks),
        Box::new(WebGallery),
    ]
}

/// Opens the repository's web page (GitHub, GitLab, ...).
pub struct OpenRepoWebsite;

impl OpenRepoWebsite {
    /// A repository target directory wins over the enclosing repository.
    fn url(ctx: &ToolContext, facts: &Facts) -> Option<String> {
        let repo_url = |facets: &[Facet]| {
            facets.iter().find_map(|f| match f {
                Facet::GitRepo { web_url, .. } => web_url.clone(),
                _ => None,
            })
        };
        let target = match (ctx.targets.as_slice(), facts.targets.as_slice()) {
            ([_], [facets]) => repo_url(facets),
            _ => None,
        };
        target.or_else(|| repo_url(&facts.dir))
    }
}

impl Tool for OpenRepoWebsite {
    fn id(&self) -> &str {
        "git.website"
    }

    fn group(&self) -> &str {
        "Git"
    }

    fn label(&self) -> &str {
        "Open repository website"
    }

    fn applies(&self, ctx: &ToolContext, facts: &Facts) -> bool {
        Self::url(ctx, facts).is_some()
    }

    fn plan(&self, inv: &Invocation) -> Result<Action, String> {
        Self::url(inv.ctx, inv.facts)
            .map(Action::OpenUrl)
            .ok_or_else(|| "No web remote".into())
    }
}

/// Lists the justfile's recipes; picking one runs it.
pub struct JustTasks;

impl Tool for JustTasks {
    fn id(&self) -> &str {
        "just.tasks"
    }

    fn group(&self) -> &str {
        "just"
    }

    fn label(&self) -> &str {
        "Run a recipe…"
    }

    fn applies(&self, _: &ToolContext, facts: &Facts) -> bool {
        facts.dir_facet(FacetKind::Justfile).is_some()
    }

    fn requires(&self) -> Vec<Requirement> {
        vec![Requirement::parse("just").unwrap()]
    }

    fn plan(&self, inv: &Invocation) -> Result<Action, String> {
        let just = inv.programs.get("just").ok_or("just not found")?;
        let root = inv
            .facts
            .dir_facet(FacetKind::Justfile)
            .ok_or("No justfile")?
            .root()
            .to_path_buf();
        let output = Command::new(just)
            .arg("--summary")
            .current_dir(&root)
            .output()
            .map_err(|e| format!("just: {e}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        let items = parse_summary(&String::from_utf8_lossy(&output.stdout))
            .into_iter()
            .map(|recipe| PickItem {
                label: recipe.clone(),
                detail: None,
                action: Action::Run(CommandSpec {
                    program: just.clone(),
                    args: vec![recipe],
                    cwd: root.clone(),
                    output: Output::Log,
                    refresh: true,
                }),
            })
            .collect();
        Ok(Action::Pick {
            title: "Recipes".into(),
            items,
        })
    }
}

/// `just --summary` prints recipe names separated by spaces.
pub fn parse_summary(summary: &str) -> Vec<String> {
    summary.split_whitespace().map(String::from).collect()
}

/// Writes an HTML page showing the selected images.
pub struct WebGallery;

impl Tool for WebGallery {
    fn id(&self) -> &str {
        "image.gallery"
    }

    fn group(&self) -> &str {
        "Image"
    }

    fn label(&self) -> &str {
        "Create web gallery…"
    }

    fn applies(&self, ctx: &ToolContext, _: &Facts) -> bool {
        ctx.targets.len() >= 2 && ctx.targets.iter().all(|t| t.kind() == FileIcon::Image)
    }

    fn params(&self, ctx: &ToolContext) -> Vec<Param> {
        vec![Param {
            key: "title".into(),
            label: "Title".into(),
            kind: ParamKind::Text {
                default: super::file_name(&ctx.dir),
            },
        }]
    }

    fn plan(&self, inv: &Invocation) -> Result<Action, String> {
        Ok(Action::Native(Box::new(GalleryJob {
            title: inv.values.get("title").cloned().unwrap_or_default(),
            images: inv.ctx.targets.iter().map(Target::name).collect(),
            output: inv.ctx.dir.join("gallery.html"),
        })))
    }
}

/// The page sits next to the images and links them by file name.
pub struct GalleryJob {
    pub title: String,
    pub images: Vec<String>,
    pub output: PathBuf,
}

impl GalleryJob {
    pub fn html(&self) -> String {
        let title = escape_html(&self.title);
        let mut html = format!(
            "<!doctype html>\n<meta charset=\"utf-8\">\n<title>{title}</title>\n<style>\
             body{{font-family:sans-serif;margin:2rem}}\
             .g{{display:grid;grid-template-columns:repeat(auto-fill,minmax(240px,1fr));gap:1rem}}\
             figure{{margin:0}}img{{width:100%;height:240px;object-fit:cover;border-radius:6px}}\
             figcaption{{font-size:.85rem;color:#666;overflow-wrap:anywhere}}</style>\n\
             <h1>{title}</h1>\n<div class=\"g\">\n"
        );
        for name in &self.images {
            let (href, text) = (escape_html(&encode_uri(name)), escape_html(name));
            let _ = writeln!(
                html,
                "<figure><a href=\"{href}\"><img src=\"{href}\" alt=\"{text}\" loading=\"lazy\"></a>\
                 <figcaption>{text}</figcaption></figure>"
            );
        }
        html.push_str("</div>\n");
        html
    }
}

impl Job for GalleryJob {
    fn describe(&self) -> String {
        format!(
            "Write {} with {} images",
            self.output.display(),
            self.images.len()
        )
    }

    fn run(self: Box<Self>) -> Result<String, String> {
        if self.output.exists() {
            return Err(format!("{} already exists", self.output.display()));
        }
        std::fs::write(&self.output, self.html()).map_err(|e| e.to_string())?;
        Ok(format!("Created {}", self.output.display()))
    }
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Percent-encodes what would break a relative URL.
fn encode_uri(name: &str) -> String {
    let mut out = String::new();
    for byte in name.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => {
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}
