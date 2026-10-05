# Tools

Context-dependent actions on files, selections and directories, grouped by
what they apply to and often backed by external programs: open a repository's
website, `uv sync`, convert images with ImageMagick, zip a selection, convert
CSV, build a web gallery from images, run `ruff`, run justfile recipes, …

Code: `src/tools/` (model, tested) and `src/tools_view.rs` + `commander.rs`
(UI). F2 opens the menu: tools grouped by section, disabled ones show the
missing program; then a parameter form if needed, a pick list for
`Action::Pick`, a confirmation listing the exact commands (URLs open without
asking), and the result in the status line or, for `output = "log"`, an
output dialog. Planning and running happen in the background.

## Flow

```
UI state ──► ToolContext ──detect──► Facts ──Registry::offers──► [Offer]  (menu)
                                                    │ user picks one, fills Params
                                                    ▼
                                    Registry::plan ──► Action  (data)
                                                    │ UI resolves Pick, confirms
                                                    ▼
                                    exec::run (background) ──► Report
                                                    │
                                                    ▼
                                    status line / log view / panel refresh
```

Every step before `exec::run` is free of side effects except reading files
and, for a few tools, quick read-only probes (e.g. `just --summary`). Plans are
therefore testable without running anything, and the UI can show exactly what
will run before it runs.

## Data model

| Type | Meaning |
|---|---|
| `ToolContext` | Panel directory, targets (marked entries or the cursor entry), other panel's directory. |
| `Target` | Path + is-dir; derived name, stem, extension and *kind* (the same `FileIcon` classification used for icons: image, code, archive, …). |
| `Facet` / `FacetKind` | Something detected about a directory by filesystem probes: `git_repo` (with web URL of `origin`), `python_project`, `uv_project`, `cargo_project`, `node_project`, `justfile`. |
| `Facts` | Facets of the panel directory (walking up, so a file deep in a repo sees the repo; nearest of each kind wins) and of each directory target (that directory only). |
| `Tool` (trait) | `id`, `group`, `label`, `applies(ctx, facts)`, `requires()`, `params(ctx)`, `plan(invocation) -> Action`. |
| `Requirement` | An external program by alias with alternatives and a hint: `"magick\|convert (ImageMagick)"`. |
| `Programs` | `PATH` lookup (with `PATHEXT` on Windows), cached; resolves requirements or reports the missing ones' hints. |
| `Offer` | A tool as shown in the menu: id, group, label, missing programs (offered disabled rather than hidden, so users learn what to install). |
| `Param` | A value asked before planning: choice or text, with a default. |
| `Action` | `OpenUrl`, `Run(CommandSpec)`, `Sequence`, `Pick { items }` (user chooses one of several actions), `Native(Job)` (in-process work). |
| `CommandSpec` | Program, args, working directory, output mode (`status`, `log`, `detached`), refresh panels afterwards. |
| `Report` | Summary line, captured log, refresh flag. |

## Two ways to define a tool

**Declarative (TOML)** for "run this command on these files", which covers
most cases. Built-ins live in `src/tools/defaults.toml`; users add or override
(by `id`) in `<config dir>/fyle/tools.toml`.

```toml
[[tool]]
id = "image.convert"
group = "Convert"
label = "Convert with ImageMagick…"
when = { kinds = ["image"] }               # all targets must match
requires = ["magick|convert (ImageMagick)"]
run = ["{magick}", "{path}", "{parent}/{stem}.{format}"]
per_target = true                          # one command per target
refresh = true
params = [{ key = "format", label = "Format", choices = ["webp", "png", "jpg"] }]
```

- `when`: `scope` (`targets` | `dir`), `entries` (`files` | `dirs` | `any`),
  `kinds` / `exts` / `names` (a target must match one, if any given),
  `min` / `max` targets, `facet` (required project facet).
- `run`: argv template, no shell. Placeholders: `{path} {name} {stem} {ext}
  {parent}` (per target), `{paths} {names}` (all targets as separate
  arguments), `{dir} {other} {root}`, program aliases, parameter keys.
  Templates are validated when loaded (unknown placeholders, misuse of list
  placeholders, `{root}` without a facet).
- `cwd`: `dir` (panel directory), `root` (project root), `parent` (target's
  directory).
- `output`: `status`, `log`, `detached`.

**Rust (`impl Tool`)** when logic is needed (`src/tools/builtin.rs`):
`git.website` (URL from `.git/config`), `just.tasks` (lists recipes, returns a
`Pick`), `image.gallery` (writes HTML in-process).

## The examples, mapped

| Example | Tool | How |
|---|---|---|
| GitHub repo → website | `git.website` | Facet `git_repo` with web URL (SSH and HTTPS remotes → `https://host/owner/repo`), `OpenUrl`. A repo directory under the cursor wins over the enclosing repo. |
| uv project basics | `uv.sync`, `uv.upgrade`, `uv.tree`, `uv.add` | `scope = "dir"`, `facet = "uv_project"` (pyproject + `uv.lock` or `[tool.uv]`), `cwd = "root"`, log output. |
| Image → convert, resize | `image.convert`, `image.resize` | `kinds = ["image"]`, ImageMagick 7 (`magick`) or 6 (`convert`). Conversions of every file type share the "Convert" group. |
| Selection → zip | `selection.zip` | `entries = "any"`, `{names}` relative to the panel directory, archive into the other panel. |
| CSV → convert | `csv.convert` | DuckDB, choice of parquet/JSON. |
| Images → web gallery | `image.gallery` | ≥ 2 images, native job writes `gallery.html` next to them. |
| Python → ruff | `python.ruff_check`, `python.ruff_format` | `exts = ["py", "pyi"]`, all selected files in one call. |
| justfile → tasks | `just.tasks` | Facet `justfile`, `just --summary`, `Pick` of recipes; each runs with log output. |

## Extending

- **New command tool:** add a `[[tool]]` to `defaults.toml` (or the user's
  `tools.toml`).
- **New logic tool:** implement `Tool`, add it to `builtin::all()`.
- **New project type:** add a `Facet`/`FacetKind` variant and a probe in
  `detect.rs`; TOML tools can then use `facet = "…"`.
- **New file kind:** extend `icons.rs` (kinds are shared with icons) or match
  by `exts`/`names`.
- **New way to run things** (e.g. open a terminal, show a dialog): add an
  `Action` variant and handle it in `exec.rs` / the UI.

## Decisions and open questions

- **No shell.** Commands are argv lists, so file names never need quoting. A
  tool that wants a shell runs `sh -c` explicitly. Exception to watch:
  arguments embedded in a program's own language, like the SQL in
  `csv.convert`, are not escaped; a `'` in a file name breaks that command.
- **Missing programs** make a tool disabled, not hidden.
- **Trust:** tools come only from fyle itself and the user's config.
  Project-local tool files (a repo shipping its own `tools.toml`) are not
  loaded; supporting them would need a trust prompt.
- **Confirmation:** every action that runs something is confirmed with the
  exact command lines; opening a URL and pick lists are not.
- **Long-running commands:** `exec` currently waits and captures output.
  Streaming output into the log view, progress and cancellation are next.
- **Windows:** `.cmd`/`.bat` programs found via `PATHEXT` are started directly;
  argument quoting for batch files is a known Rust/Windows caveat. `zip` is
  usually missing on Windows, so a native zip job may be worth adding.
- **Performance:** detection reads a few files per ancestor directory;
  offers should be computed in the background when the menu opens.
