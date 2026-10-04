//! Tools: context-dependent actions on files, selections and directories,
//! often backed by external programs. UI-independent; see `docs/tools.md`.
//!
//! Flow:
//! 1. The UI builds a [`ToolContext`] from the active panel (its directory,
//!    the marked entries or the cursor entry, the other panel's directory).
//! 2. [`detect::detect`] derives [`Facts`]: project facets of the directory
//!    and of target directories (git repo, uv project, justfile, ...).
//! 3. [`Registry::offers`] lists the tools that apply, grouped, each marked
//!    ready or missing a required program.
//! 4. After the user picks one (and fills in its [`Param`]s), [`Registry::plan`]
//!    asks the tool for an [`Action`]: plain data describing what to do.
//! 5. The UI shows/confirms the action and hands it to [`exec`] to run, in the
//!    background.
//!
//! Tools come in two forms: declarative [`command::CommandTool`]s (defined in
//! TOML: built-in `defaults.toml` plus the user's `tools.toml`) and Rust
//! implementations of [`Tool`] for anything needing logic ([`builtin`]).

pub mod builtin;
pub mod command;
pub mod detect;
pub mod exec;
pub mod programs;

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::icons::{self, FileIcon};
pub use programs::{Programs, Requirement, Resolved};

/// A file or directory a tool acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub path: PathBuf,
    pub is_dir: bool,
}

impl Target {
    pub fn new(path: impl Into<PathBuf>, is_dir: bool) -> Self {
        Self {
            path: path.into(),
            is_dir,
        }
    }

    pub fn name(&self) -> String {
        file_name(&self.path)
    }

    /// File name without the last extension (`a.tar.gz` → `a.tar`).
    pub fn stem(&self) -> String {
        self.path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// Lowercase extension without the dot; empty for directories.
    pub fn ext(&self) -> String {
        if self.is_dir {
            return String::new();
        }
        self.path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    }

    pub fn kind(&self) -> FileIcon {
        icons::icon_for(&self.name(), self.is_dir, false, false)
    }

    pub fn parent(&self) -> PathBuf {
        self.path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default()
    }
}

pub(crate) fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// What the tools see of the UI state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolContext {
    /// The active panel's directory.
    pub dir: PathBuf,
    /// Marked entries, or the cursor entry when nothing is marked.
    pub targets: Vec<Target>,
    /// The other panel's directory (a natural output location).
    pub other_dir: Option<PathBuf>,
}

/// Kinds of [`Facet`], as named in tool definitions (`facet = "uv_project"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FacetKind {
    GitRepo,
    PythonProject,
    UvProject,
    CargoProject,
    NodeProject,
    Justfile,
}

/// Something detected about a directory, found by cheap filesystem probes
/// (no external programs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Facet {
    GitRepo {
        root: PathBuf,
        /// Browser URL of the `origin` remote, if it is a web host.
        web_url: Option<String>,
    },
    PythonProject {
        root: PathBuf,
    },
    UvProject {
        root: PathBuf,
    },
    CargoProject {
        root: PathBuf,
    },
    NodeProject {
        root: PathBuf,
    },
    Justfile {
        path: PathBuf,
    },
}

impl Facet {
    pub fn kind(&self) -> FacetKind {
        match self {
            Self::GitRepo { .. } => FacetKind::GitRepo,
            Self::PythonProject { .. } => FacetKind::PythonProject,
            Self::UvProject { .. } => FacetKind::UvProject,
            Self::CargoProject { .. } => FacetKind::CargoProject,
            Self::NodeProject { .. } => FacetKind::NodeProject,
            Self::Justfile { .. } => FacetKind::Justfile,
        }
    }

    /// The directory the facet belongs to (a justfile's directory).
    pub fn root(&self) -> &Path {
        match self {
            Self::GitRepo { root, .. }
            | Self::PythonProject { root }
            | Self::UvProject { root }
            | Self::CargoProject { root }
            | Self::NodeProject { root } => root,
            Self::Justfile { path } => path.parent().unwrap_or(path),
        }
    }
}

/// Facets of the context: of the panel directory (including enclosing
/// projects, nearest first) and of each directory target.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    pub dir: Vec<Facet>,
    /// Parallel to [`ToolContext::targets`]; empty for files.
    pub targets: Vec<Vec<Facet>>,
}

impl Facts {
    /// The panel directory's facet of `kind`, if any.
    pub fn dir_facet(&self, kind: FacetKind) -> Option<&Facet> {
        self.dir.iter().find(|f| f.kind() == kind)
    }
}

/// A value the user supplies before a tool runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub key: String,
    pub label: String,
    pub kind: ParamKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamKind {
    /// One of `options`; the first is the default.
    Choice(Vec<String>),
    Text {
        default: String,
    },
}

impl Param {
    pub fn default_value(&self) -> String {
        match &self.kind {
            ParamKind::Choice(options) => options.first().cloned().unwrap_or_default(),
            ParamKind::Text { default } => default.clone(),
        }
    }
}

/// Parameter values by key.
pub type Values = HashMap<String, String>;

/// Everything a tool gets when asked to plan.
pub struct Invocation<'a> {
    pub ctx: &'a ToolContext,
    pub facts: &'a Facts,
    /// Resolved [`Tool::requires`] programs, by alias.
    pub programs: &'a Resolved,
    pub values: &'a Values,
}

/// A context-dependent action.
pub trait Tool: Send + Sync {
    /// Stable identifier, e.g. `"image.convert"`; user definitions with the
    /// same id replace built-in ones.
    fn id(&self) -> &str;
    /// Menu group, e.g. `"Image"`, `"Git"`, `"uv"`.
    fn group(&self) -> &str;
    fn label(&self) -> &str;
    /// Whether the tool is offered for this context.
    fn applies(&self, ctx: &ToolContext, facts: &Facts) -> bool;
    /// External programs needed; a tool missing one is offered disabled.
    fn requires(&self) -> Vec<Requirement> {
        Vec::new()
    }
    /// Values to ask the user for before planning.
    fn params(&self, _ctx: &ToolContext) -> Vec<Param> {
        Vec::new()
    }
    /// Describes what to do. May block briefly (reading files, quick probes
    /// such as `just --summary`), so callers run it off the UI thread.
    fn plan(&self, inv: &Invocation) -> Result<Action, String>;
}

/// What running a tool does, as data. Executed by [`exec`].
pub enum Action {
    OpenUrl(String),
    Run(CommandSpec),
    /// Run in order, stopping at the first failure.
    Sequence(Vec<Action>),
    /// Let the user choose one of several actions (e.g. justfile recipes).
    Pick {
        title: String,
        items: Vec<PickItem>,
    },
    /// Work done in-process (e.g. generating a web gallery).
    Native(Box<dyn Job>),
}

pub struct PickItem {
    pub label: String,
    pub detail: Option<String>,
    pub action: Action,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub output: Output,
    /// Reload the panels afterwards (the command changed files).
    pub refresh: bool,
}

/// How a command's result is presented.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Output {
    /// Wait; report success or the error in the status line.
    #[default]
    Status,
    /// Wait; show stdout/stderr in a log view.
    Log,
    /// Start and do not wait (GUI programs, long-running servers).
    Detached,
}

/// In-process work.
pub trait Job: Send {
    /// One line for confirmations and logs.
    fn describe(&self) -> String;
    /// Does the work; returns a summary for the status line.
    fn run(self: Box<Self>) -> Result<String, String>;
    /// Whether files changed (panels should reload).
    fn refresh(&self) -> bool {
        true
    }
}

impl fmt::Debug for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OpenUrl(url) => f.debug_tuple("OpenUrl").field(url).finish(),
            Self::Run(spec) => f.debug_tuple("Run").field(spec).finish(),
            Self::Sequence(actions) => f.debug_tuple("Sequence").field(actions).finish(),
            Self::Pick { title, items } => f
                .debug_struct("Pick")
                .field("title", title)
                .field("items", &items.iter().map(|i| &i.label).collect::<Vec<_>>())
                .finish(),
            Self::Native(job) => f.debug_tuple("Native").field(&job.describe()).finish(),
        }
    }
}

impl Action {
    /// Lines describing what will happen, for a confirmation prompt.
    pub fn describe(&self) -> Vec<String> {
        match self {
            Self::OpenUrl(url) => vec![format!("Open {url}")],
            Self::Run(spec) => vec![spec.command_line()],
            Self::Sequence(actions) => actions.iter().flat_map(Action::describe).collect(),
            Self::Pick { title, items } => vec![format!("{title}: {} choices", items.len())],
            Self::Native(job) => vec![job.describe()],
        }
    }

    /// Whether running it changes nothing locally, so no confirmation is needed.
    pub fn is_harmless(&self) -> bool {
        matches!(self, Self::OpenUrl(_) | Self::Pick { .. })
    }
}

impl CommandSpec {
    /// Shell-like rendering for display only (commands never run in a shell).
    pub fn command_line(&self) -> String {
        let quote = |s: &str| {
            if !s.is_empty()
                && s.chars()
                    .all(|c| c.is_alphanumeric() || "-_./=:,+@%".contains(c))
            {
                s.to_string()
            } else {
                format!("'{}'", s.replace('\'', "'\\''"))
            }
        };
        let program = self
            .program
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.program.display().to_string());
        std::iter::once(quote(&program))
            .chain(self.args.iter().map(|a| quote(a)))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// A tool as offered in the menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub id: String,
    pub group: String,
    pub label: String,
    /// Empty when ready; otherwise hints for the missing programs.
    pub missing: Vec<String>,
}

#[derive(Default)]
pub struct Registry {
    tools: Vec<Box<dyn Tool>>,
}

impl Registry {
    /// Built-in Rust tools plus the declarative `defaults.toml`.
    pub fn with_defaults() -> Self {
        let mut registry = Self::default();
        for tool in builtin::all() {
            registry.register(tool);
        }
        registry
            .load_toml(include_str!("defaults.toml"))
            .expect("built-in tools.toml is valid");
        registry
    }

    /// Adds a tool; one with the same id is replaced in place.
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        match self.tools.iter().position(|t| t.id() == tool.id()) {
            Some(ix) => self.tools[ix] = tool,
            None => self.tools.push(tool),
        }
    }

    /// Registers the `[[tool]]` definitions in `text` (TOML).
    pub fn load_toml(&mut self, text: &str) -> Result<(), String> {
        for tool in command::parse(text)? {
            self.register(Box::new(tool));
        }
        Ok(())
    }

    /// Applicable tools, grouped (groups in registration order).
    pub fn offers(&self, ctx: &ToolContext, facts: &Facts, programs: &Programs) -> Vec<Offer> {
        let mut groups: Vec<&str> = Vec::new();
        let mut offers: Vec<Offer> = Vec::new();
        for tool in self.tools.iter().filter(|t| t.applies(ctx, facts)) {
            if !groups.contains(&tool.group()) {
                groups.push(tool.group());
            }
            offers.push(Offer {
                id: tool.id().to_string(),
                group: tool.group().to_string(),
                label: tool.label().to_string(),
                missing: programs.resolve(&tool.requires()).err().unwrap_or_default(),
            });
        }
        offers.sort_by_key(|o| groups.iter().position(|g| *g == o.group));
        offers
    }

    pub fn tool(&self, id: &str) -> Option<&dyn Tool> {
        self.tools.iter().find(|t| t.id() == id).map(|t| t.as_ref())
    }

    /// Plans tool `id`; missing parameter values take their defaults.
    pub fn plan(
        &self,
        id: &str,
        ctx: &ToolContext,
        facts: &Facts,
        programs: &Programs,
        values: &Values,
    ) -> Result<Action, String> {
        let tool = self.tool(id).ok_or_else(|| format!("Unknown tool {id}"))?;
        if !tool.applies(ctx, facts) {
            return Err(format!("{} does not apply here", tool.label()));
        }
        let resolved = programs
            .resolve(&tool.requires())
            .map_err(|missing| format!("Requires {}", missing.join(", ")))?;
        let mut values = values.clone();
        for param in tool.params(ctx) {
            values
                .entry(param.key.clone())
                .or_insert_with(|| param.default_value());
        }
        tool.plan(&Invocation {
            ctx,
            facts,
            programs: &resolved,
            values: &values,
        })
    }
}

#[cfg(test)]
mod tests;
