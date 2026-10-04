//! Declarative tools: an external command template plus matching rules,
//! defined in TOML (built-in `defaults.toml`, the user's `tools.toml`).
//!
//! ```toml
//! [[tool]]
//! id = "image.convert"
//! group = "Image"
//! label = "Convert with ImageMagick…"
//! when = { kinds = ["image"] }          # every target must be an image
//! requires = ["magick|convert (ImageMagick)"]
//! run = ["{magick}", "{path}", "{parent}/{stem}.{format}"]
//! per_target = true                     # one command per target
//! refresh = true
//! params = [{ key = "format", label = "Format", choices = ["webp", "png", "jpg"] }]
//! ```
//!
//! Placeholders (each must be a whole argument or part of one):
//! - target: `{path}` `{name}` `{stem}` `{ext}` `{parent}` (with `per_target`,
//!   or a single target)
//! - all targets, as separate arguments (whole argument only): `{paths}` `{names}`
//! - context: `{dir}` (panel directory), `{other}` (other panel's directory),
//!   `{root}` (root of the `when.facet` project)
//! - programs: one per `requires` alias, e.g. `{magick}`
//! - parameters: one per `params` key

use serde::Deserialize;

use super::{
    Action, CommandSpec, FacetKind, Facts, Invocation, Output, Param, ParamKind, Requirement,
    Target, Tool, ToolContext,
};
use crate::icons::FileIcon;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolsFile {
    #[serde(default)]
    tool: Vec<CommandToolDef>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandToolDef {
    id: String,
    group: String,
    label: String,
    #[serde(default)]
    when: When,
    #[serde(default)]
    requires: Vec<String>,
    run: Vec<String>,
    #[serde(default)]
    per_target: bool,
    #[serde(default)]
    cwd: Cwd,
    #[serde(default)]
    output: Output,
    #[serde(default)]
    refresh: bool,
    #[serde(default)]
    params: Vec<ParamDef>,
}

/// When a tool is offered.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct When {
    /// `targets` (default): acts on marked entries / the cursor entry.
    /// `dir`: acts on the panel directory, independent of the cursor.
    pub scope: Scope,
    /// Which targets qualify (default `files`).
    pub entries: Entries,
    /// Targets must match one of `kinds`, `exts` or `names` when any is given.
    pub kinds: Vec<FileIcon>,
    pub exts: Vec<String>,
    pub names: Vec<String>,
    pub min: usize,
    pub max: Option<usize>,
    /// Required facet of the panel directory (or an enclosing directory).
    pub facet: Option<FacetKind>,
}

impl Default for When {
    fn default() -> Self {
        Self {
            scope: Scope::Targets,
            entries: Entries::Files,
            kinds: Vec::new(),
            exts: Vec::new(),
            names: Vec::new(),
            min: 1,
            max: None,
            facet: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    #[default]
    Targets,
    Dir,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Entries {
    #[default]
    Files,
    Dirs,
    Any,
}

/// Working directory of the command.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cwd {
    /// The panel directory.
    #[default]
    Dir,
    /// Root of the `when.facet` project.
    Root,
    /// The target's directory (with `per_target`).
    Parent,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParamDef {
    key: String,
    label: String,
    #[serde(default)]
    choices: Vec<String>,
    #[serde(default)]
    default: String,
}

pub struct CommandTool {
    def: CommandToolDef,
    requires: Vec<Requirement>,
}

/// Parses and validates the `[[tool]]` entries of a TOML document.
pub fn parse(text: &str) -> Result<Vec<CommandTool>, String> {
    let file: ToolsFile = toml::from_str(text).map_err(|e| e.message().to_string())?;
    file.tool.into_iter().map(CommandTool::new).collect()
}

const TARGET_VARS: &[&str] = &["path", "name", "stem", "ext", "parent"];
const LIST_VARS: &[&str] = &["paths", "names"];
const CONTEXT_VARS: &[&str] = &["dir", "other", "root"];

impl CommandTool {
    fn new(def: CommandToolDef) -> Result<Self, String> {
        let error = |msg: String| format!("tool {:?}: {msg}", def.id);
        let mut requires = def
            .requires
            .iter()
            .map(|r| Requirement::parse(r))
            .collect::<Result<Vec<_>, _>>()
            .map_err(error)?;
        let program = def.run.first().ok_or_else(|| error("empty `run`".into()))?;
        // A literal program name is an implicit requirement.
        if !program.starts_with('{') {
            requires.push(Requirement::parse(program).map_err(error)?);
        }
        let known = |name: &str| {
            TARGET_VARS.contains(&name)
                || LIST_VARS.contains(&name)
                || CONTEXT_VARS.contains(&name)
                || requires.iter().any(|r| r.alias == name)
                || def.params.iter().any(|p| p.key == name)
        };
        for arg in &def.run {
            for name in placeholders(arg).map_err(error)? {
                if !known(name) {
                    return Err(error(format!("unknown placeholder {{{name}}}")));
                }
                if name == "root" && def.when.facet.is_none() {
                    return Err(error("{root} needs `when.facet`".into()));
                }
                if LIST_VARS.contains(&name) && arg != &format!("{{{name}}}") {
                    return Err(error(format!("{{{name}}} must be a whole argument")));
                }
            }
        }
        if def.cwd == Cwd::Root && def.when.facet.is_none() {
            return Err(error("cwd = \"root\" needs `when.facet`".into()));
        }
        Ok(Self { def, requires })
    }

    fn target_matches(&self, target: &Target) -> bool {
        let when = &self.def.when;
        let entry_ok = match when.entries {
            Entries::Files => !target.is_dir,
            Entries::Dirs => target.is_dir,
            Entries::Any => true,
        };
        let filtered = !when.kinds.is_empty() || !when.exts.is_empty() || !when.names.is_empty();
        entry_ok
            && (!filtered
                || when.kinds.contains(&target.kind())
                || when.exts.contains(&target.ext())
                || when.names.contains(&target.name()))
    }

    /// Expands every `run` argument for one command.
    fn expand(&self, inv: &Invocation, target: Option<&Target>) -> Result<Vec<String>, String> {
        let ctx = inv.ctx;
        let root = self
            .def
            .when
            .facet
            .and_then(|kind| inv.facts.dir_facet(kind))
            .map(|f| f.root().display().to_string());
        let lookup = |name: &str| -> Result<Value, String> {
            let one = |s: String| Ok(Value::One(s));
            match name {
                "paths" => Ok(Value::Many(
                    ctx.targets
                        .iter()
                        .map(|t| t.path.display().to_string())
                        .collect(),
                )),
                "names" => Ok(Value::Many(ctx.targets.iter().map(Target::name).collect())),
                "dir" => one(ctx.dir.display().to_string()),
                "other" => ctx
                    .other_dir
                    .as_ref()
                    .map(|d| Value::One(d.display().to_string()))
                    .ok_or_else(|| "no other panel directory".to_string()),
                "root" => root
                    .clone()
                    .map(Value::One)
                    .ok_or_else(|| "no project root".into()),
                _ if TARGET_VARS.contains(&name) => {
                    let t = target.ok_or_else(|| format!("{{{name}}} needs a single target"))?;
                    one(match name {
                        "path" => t.path.display().to_string(),
                        "name" => t.name(),
                        "stem" => t.stem(),
                        "ext" => t.ext(),
                        _ => t.parent().display().to_string(),
                    })
                }
                _ => inv
                    .programs
                    .get(name)
                    .map(|p| p.display().to_string())
                    .or_else(|| inv.values.get(name).cloned())
                    .map(Value::One)
                    .ok_or_else(|| format!("no value for {{{name}}}")),
            }
        };
        let mut args = Vec::new();
        for arg in &self.def.run {
            args.extend(expand_arg(arg, &lookup)?);
        }
        Ok(args)
    }

    fn command(&self, inv: &Invocation, target: Option<&Target>) -> Result<CommandSpec, String> {
        let mut args = self.expand(inv, target)?;
        let first = args.remove(0);
        // Literal program names run the resolved executable.
        let program = inv
            .programs
            .get(&first)
            .cloned()
            .unwrap_or_else(|| first.into());
        let cwd = match self.def.cwd {
            Cwd::Dir => inv.ctx.dir.clone(),
            Cwd::Root => self
                .def
                .when
                .facet
                .and_then(|kind| inv.facts.dir_facet(kind))
                .map(|f| f.root().to_path_buf())
                .ok_or("no project root")?,
            Cwd::Parent => target
                .map(Target::parent)
                .ok_or("cwd = \"parent\" needs a target")?,
        };
        Ok(CommandSpec {
            program,
            args,
            cwd,
            output: self.def.output,
            refresh: self.def.refresh,
        })
    }
}

impl Tool for CommandTool {
    fn id(&self) -> &str {
        &self.def.id
    }

    fn group(&self) -> &str {
        &self.def.group
    }

    fn label(&self) -> &str {
        &self.def.label
    }

    fn applies(&self, ctx: &ToolContext, facts: &Facts) -> bool {
        let when = &self.def.when;
        if when
            .facet
            .is_some_and(|kind| facts.dir_facet(kind).is_none())
        {
            return false;
        }
        match when.scope {
            Scope::Dir => true,
            Scope::Targets => {
                let n = ctx.targets.len();
                n >= when.min.max(1)
                    && when.max.is_none_or(|max| n <= max)
                    && ctx.targets.iter().all(|t| self.target_matches(t))
            }
        }
    }

    fn requires(&self) -> Vec<Requirement> {
        self.requires.clone()
    }

    fn params(&self, _: &ToolContext) -> Vec<Param> {
        self.def
            .params
            .iter()
            .map(|p| Param {
                key: p.key.clone(),
                label: p.label.clone(),
                kind: if p.choices.is_empty() {
                    ParamKind::Text {
                        default: p.default.clone(),
                    }
                } else {
                    ParamKind::Choice(p.choices.clone())
                },
            })
            .collect()
    }

    fn plan(&self, inv: &Invocation) -> Result<Action, String> {
        let targets = &inv.ctx.targets;
        if self.def.per_target && self.def.when.scope == Scope::Targets {
            let mut commands = targets
                .iter()
                .map(|t| self.command(inv, Some(t)).map(Action::Run))
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(if commands.len() == 1 {
                commands.remove(0)
            } else {
                Action::Sequence(commands)
            });
        }
        let single = (targets.len() == 1).then(|| &targets[0]);
        Ok(Action::Run(self.command(inv, single)?))
    }
}

enum Value {
    One(String),
    Many(Vec<String>),
}

/// Names of the `{...}` placeholders in `arg`.
fn placeholders(arg: &str) -> Result<Vec<&str>, String> {
    let mut names = Vec::new();
    let mut rest = arg;
    while let Some(start) = rest.find('{') {
        let end = rest[start..]
            .find('}')
            .ok_or_else(|| format!("unclosed placeholder in {arg:?}"))?;
        names.push(&rest[start + 1..start + end]);
        rest = &rest[start + end + 1..];
    }
    Ok(names)
}

fn expand_arg(
    arg: &str,
    lookup: &dyn Fn(&str) -> Result<Value, String>,
) -> Result<Vec<String>, String> {
    if let Some(name) = arg.strip_prefix('{').and_then(|a| a.strip_suffix('}'))
        && !name.contains(['{', '}'])
        && let Value::Many(values) = lookup(name)?
    {
        return Ok(values);
    }
    let mut out = String::new();
    let mut rest = arg;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let end = start + rest[start..].find('}').ok_or("unclosed placeholder")?;
        match lookup(&rest[start + 1..end])? {
            Value::One(value) => out.push_str(&value),
            Value::Many(_) => return Err(format!("{arg:?}: list placeholder inside text")),
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    Ok(vec![out])
}
