//! Tools defined in Lua plugins: `<config dir>/fyle/plugins/*.lua`. Same
//! trust rule as `tools.toml`: only the user's own config is loaded.
//!
//! A plugin returns a list of tools:
//!
//! ```lua
//! return {
//!   {
//!     id = "md.pdf",
//!     group = "Markdown",
//!     label = "Convert to PDF…",
//!     requires = { "pandoc" },
//!     params = { { key = "engine", label = "Engine", choices = { "typst", "xelatex" } } },
//!     applies = function(ctx)
//!       return #ctx.targets == 1 and ctx.targets[1].ext == "md"
//!     end,
//!     plan = function(ctx, values)
//!       local t = ctx.targets[1]
//!       return { run = { ctx.programs.pandoc, t.path, "--pdf-engine=" .. values.engine,
//!                        "-o", t.parent .. "/" .. t.stem .. ".pdf" },
//!                refresh = true }
//!     end,
//!   },
//! }
//! ```
//!
//! `ctx`: `dir`, `other` (other panel's directory or nil), `targets` (each
//! with `path`, `name`, `stem`, `ext`, `parent`, `is_dir`, `kind`), `facets`
//! (facet kind → project root, e.g. `facets.git_repo`) and, in `plan`,
//! `programs` (`requires` alias → resolved path). `applies` is optional
//! (default: always offered).
//!
//! `plan` returns an action table:
//! - `{ run = {program, args…}, cwd = dir, output = "status"|"log"|"detached", refresh = bool }`
//! - `{ open_url = "https://…" }`
//! - `{ sequence = { action, … } }`
//! - `{ pick = { title = "…", items = { { label = "…", detail = "…", action = action }, … } } }`
//!
//! or raises an error (`error("…")`), shown in the status line.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use mlua::{Function, Lua, RegistryKey, Table, Value};

use super::{
    Action, CommandSpec, Facts, Invocation, Output, Param, ParamKind, PickItem, Requirement, Tool,
    ToolContext,
};

/// One tool from a plugin. Tools of a file share its Lua state.
pub struct LuaTool {
    lua: Arc<Mutex<Lua>>,
    /// The tool's definition table.
    def: RegistryKey,
    id: String,
    group: String,
    label: String,
    requires: Vec<Requirement>,
    params: Vec<Param>,
}

/// Runs the plugin `source` (named `name` in errors) and returns its tools.
pub fn load(name: &str, source: &str) -> Result<Vec<LuaTool>, String> {
    let lua = Lua::new();
    let error = |e: mlua::Error| format!("{name}: {e}");
    let defs: Table = lua.load(source).set_name(name).eval().map_err(error)?;
    let lua = Arc::new(Mutex::new(lua));
    let mut tools = Vec::new();
    for (ix, def) in defs.sequence_values::<Table>().enumerate() {
        let def = def.map_err(error)?;
        let tool = LuaTool::new(&lua, def).map_err(|e| format!("{name}: tool {}: {e}", ix + 1))?;
        tools.push(tool);
    }
    Ok(tools)
}

/// Loads every `*.lua` in `dir`, in name order. Returns the tools and an
/// error per plugin that failed; a missing directory is no error.
pub fn load_dir(dir: &Path) -> (Vec<LuaTool>, Vec<String>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return (Vec::new(), Vec::new());
    };
    let mut paths: Vec<PathBuf> = read
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "lua"))
        .collect();
    paths.sort();
    let (mut tools, mut errors) = (Vec::new(), Vec::new());
    for path in paths {
        let name = super::file_name(&path);
        match std::fs::read_to_string(&path) {
            Ok(source) => match load(&name, &source) {
                Ok(loaded) => tools.extend(loaded),
                Err(e) => errors.push(e),
            },
            Err(e) => errors.push(format!("{name}: {e}")),
        }
    }
    (tools, errors)
}

impl LuaTool {
    fn new(lua: &Arc<Mutex<Lua>>, def: Table) -> Result<Self, String> {
        let text = |key: &str| -> Result<String, String> {
            def.get::<Option<String>>(key)
                .map_err(|e| format!("`{key}`: {e}"))?
                .filter(|s| !s.is_empty())
                .ok_or_else(|| format!("missing `{key}`"))
        };
        let (id, group, label) = (text("id")?, text("group")?, text("label")?);
        let id_error = |e: String| format!("{id}: {e}");
        if !matches!(def.get::<Value>("plan"), Ok(Value::Function(_))) {
            return Err(id_error("missing `plan` function".into()));
        }
        let requires = def
            .get::<Option<Vec<String>>>("requires")
            .map_err(|e| id_error(format!("`requires`: {e}")))?
            .unwrap_or_default()
            .iter()
            .map(|spec| Requirement::parse(spec))
            .collect::<Result<_, _>>()
            .map_err(id_error)?;
        let params = parse_params(&def).map_err(id_error)?;
        let key = lua
            .lock()
            .expect("Lua lock is never poisoned")
            .create_registry_value(def)
            .map_err(|e| id_error(e.to_string()))?;
        Ok(Self {
            lua: lua.clone(),
            def: key,
            id,
            group,
            label,
            requires,
            params,
        })
    }

    /// Calls the definition's function `name` with a context table.
    fn call<R: mlua::FromLuaMulti>(
        &self,
        name: &str,
        ctx: &ToolContext,
        facts: &Facts,
        extra: impl FnOnce(&Lua, &Table) -> mlua::Result<Option<Table>>,
    ) -> Result<Option<R>, String> {
        let lua = self.lua.lock().expect("Lua lock is never poisoned");
        let run = || -> mlua::Result<Option<R>> {
            let def: Table = lua.registry_value(&self.def)?;
            let Some(function) = def.get::<Option<Function>>(name)? else {
                return Ok(None);
            };
            let table = context_table(&lua, ctx, facts)?;
            let second = extra(&lua, &table)?;
            function.call((table, second)).map(Some)
        };
        run().map_err(|e| format!("{}: {e}", self.id))
    }
}

fn parse_params(def: &Table) -> Result<Vec<Param>, String> {
    let Some(list) = def
        .get::<Option<Vec<Table>>>("params")
        .map_err(|e| format!("`params`: {e}"))?
    else {
        return Ok(Vec::new());
    };
    list.into_iter()
        .map(|p| {
            let key: String = p.get("key").map_err(|e| format!("param `key`: {e}"))?;
            let label = p.get::<Option<String>>("label").ok().flatten();
            let choices = p
                .get::<Option<Vec<String>>>("choices")
                .map_err(|e| format!("param {key}: `choices`: {e}"))?;
            let kind = match choices {
                Some(options) if !options.is_empty() => ParamKind::Choice(options),
                _ => ParamKind::Text {
                    default: p
                        .get::<Option<String>>("default")
                        .ok()
                        .flatten()
                        .unwrap_or_default(),
                },
            };
            Ok(Param {
                label: label.unwrap_or_else(|| key.clone()),
                key,
                kind,
            })
        })
        .collect()
}

/// `Debug` name in snake case: `GitRepo` → `git_repo`.
fn snake(name: impl std::fmt::Debug) -> String {
    let mut out = String::new();
    for (i, c) in format!("{name:?}").chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push('_');
        }
        out.extend(c.to_lowercase());
    }
    out
}

fn context_table(lua: &Lua, ctx: &ToolContext, facts: &Facts) -> mlua::Result<Table> {
    let path = |p: &Path| p.to_string_lossy().into_owned();
    let table = lua.create_table()?;
    table.set("dir", path(&ctx.dir))?;
    table.set("other", ctx.other_dir.as_deref().map(path))?;
    let targets = lua.create_table()?;
    for target in &ctx.targets {
        let t = lua.create_table()?;
        t.set("path", path(&target.path))?;
        t.set("name", target.name())?;
        t.set("stem", target.stem())?;
        t.set("ext", target.ext())?;
        t.set("parent", path(&target.parent()))?;
        t.set("is_dir", target.is_dir)?;
        t.set("kind", snake(target.kind()))?;
        targets.push(t)?;
    }
    table.set("targets", targets)?;
    let facets = lua.create_table()?;
    // Nearest first: keep the first facet of each kind.
    for facet in facts.dir.iter().rev() {
        facets.set(snake(facet.kind()), path(facet.root()))?;
    }
    table.set("facets", facets)?;
    Ok(table)
}

/// Converts a returned action table.
fn action(value: Value, cwd: &Path) -> Result<Action, String> {
    let Value::Table(t) = value else {
        return Err(format!(
            "plan must return an action table, not {}",
            value.type_name()
        ));
    };
    let get = |key: &str| t.get::<Value>(key).map_err(|e| e.to_string());
    if let Value::String(url) = get("open_url")? {
        return Ok(Action::OpenUrl(
            url.to_str().map_err(|e| e.to_string())?.to_string(),
        ));
    }
    if let Value::Table(list) = get("sequence")? {
        return list
            .sequence_values::<Value>()
            .map(|a| action(a.map_err(|e| e.to_string())?, cwd))
            .collect::<Result<_, _>>()
            .map(Action::Sequence);
    }
    if let Value::Table(pick) = get("pick")? {
        let title = pick
            .get::<Option<String>>("title")
            .ok()
            .flatten()
            .unwrap_or_default();
        let items: Vec<Table> = pick.get("items").map_err(|e| format!("pick.items: {e}"))?;
        let items = items
            .into_iter()
            .map(|item| {
                Ok(PickItem {
                    label: item
                        .get("label")
                        .map_err(|e| format!("pick item label: {e}"))?,
                    detail: item.get::<Option<String>>("detail").ok().flatten(),
                    action: action(item.get("action").map_err(|e| e.to_string())?, cwd)?,
                })
            })
            .collect::<Result<_, String>>()?;
        return Ok(Action::Pick { title, items });
    }
    if let Value::Table(_) = get("run")? {
        let run: Vec<String> = t.get("run").map_err(|e| format!("run: {e}"))?;
        let Some((program, args)) = run.split_first() else {
            return Err("run is empty".into());
        };
        let output = match t
            .get::<Option<String>>("output")
            .map_err(|e| e.to_string())?
        {
            None => Output::Status,
            Some(o) => match o.as_str() {
                "status" => Output::Status,
                "log" => Output::Log,
                "detached" => Output::Detached,
                other => return Err(format!("unknown output {other:?}")),
            },
        };
        let dir: Option<String> = t.get("cwd").map_err(|e| e.to_string())?;
        return Ok(Action::Run(CommandSpec {
            program: PathBuf::from(program),
            args: args.to_vec(),
            cwd: dir.map_or_else(|| cwd.to_path_buf(), PathBuf::from),
            output,
            refresh: t
                .get::<Option<bool>>("refresh")
                .ok()
                .flatten()
                .unwrap_or(false),
        }));
    }
    Err("action needs one of run, open_url, sequence, pick".into())
}

impl Tool for LuaTool {
    fn id(&self) -> &str {
        &self.id
    }

    fn group(&self) -> &str {
        &self.group
    }

    fn label(&self) -> &str {
        &self.label
    }

    fn applies(&self, ctx: &ToolContext, facts: &Facts) -> bool {
        // A failing `applies` hides the tool rather than breaking the menu.
        self.call::<bool>("applies", ctx, facts, |_, _| Ok(None))
            .map(|r| r.unwrap_or(true))
            .unwrap_or(false)
    }

    fn requires(&self) -> Vec<Requirement> {
        self.requires.clone()
    }

    fn params(&self, _: &ToolContext) -> Vec<Param> {
        self.params.clone()
    }

    fn plan(&self, inv: &Invocation) -> Result<Action, String> {
        let value = self
            .call::<Value>("plan", inv.ctx, inv.facts, |lua, ctx| {
                let programs = lua.create_table()?;
                for (alias, path) in inv.programs {
                    programs.set(alias.as_str(), path.to_string_lossy().into_owned())?;
                }
                ctx.set("programs", programs)?;
                let values = lua.create_table()?;
                for (key, value) in inv.values {
                    values.set(key.as_str(), value.as_str())?;
                }
                Ok(Some(values))
            })?
            .unwrap_or(Value::Nil);
        action(value, &inv.ctx.dir).map_err(|e| format!("{}: {e}", self.id))
    }
}
