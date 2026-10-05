# Handoff: fyle

The current state and the plan, for whoever continues this work.
`TODO.md` is the live backlog. `README.md` covers usage, settings and
architecture. `docs/tools.md` covers the tools design.

## Goal

A simple, Krusader-like file manager with two panels, built on GPUI Kit
(`gpui-kit` 0.7.0, which bundles gpui, gpui-component and the default
assets). Licensed GPL-3.0-or-later. It targets Linux now. Windows is
paused and macOS comes later.

## Working agreements with the owner

- Answers should be short, precise and honest; say so when unsure.
- Make one feature per commit, and push each one. The branch is
  `ccr-bbf121a9-oitqaj`. Don't open a PR unless asked.
- Commit trailers:
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` and
  `Claude-Session: https://claude.ai/code/session_01RziBhqhvJeK9PsGz3Zr5f5`
  (or the new session's link).
- Write the model first: every feature gets a UI-independent model with
  unit tests, and a thin view on top.
- Don't vendor big asset sets. Icons come only from GPUI Kit's default
  bundle. Themes are the 21 JSON files copied from gpui-kit (Apache-2.0,
  attributed in `themes/README.md`).
- Windows is ignored for now. There's no Windows CI and no cross-checks.
  Keep `#[cfg(windows)]` code compiling where it already exists, but
  don't extend it.
- Search uses ripgrep's libraries only (`ignore`, `grep-*`), never the
  `rg` or `fd` binaries.
- Lua plugins are approved in principle: `mlua` with `lua54` and
  `vendored`.

## Status (46 commits, about 11.3k lines of Rust, 89 tests passing)

Done (details in `TODO.md` under "Done"):

- **Panels:** DataTable panels, tree mode, natural sort, configurable
  columns (incl. created time, git status, xattrs, SELinux), filter as
  you type. Dev builds optimise dependencies (unoptimised gpui lagged).
- **File operations:** copy, move and trash run as background jobs with
  progress and cancel. Also rename and bulk rename, and yank/cut/paste.
- **Viewing and editing:** F3 preview covers text (highlighted), images,
  directories, hex, Markdown, CSV tables, archive listings, PDF (first
  page via poppler), EXIF, and audio/video details with a video frame
  (ffmpeg). F4 is the
  built-in editor, with an optional external editor; Shift-F4 creates a
  new file.
- **Navigation:** find files and search contents (Alt-F7; literal or
  regex, hits show the matching line). F9 opens a terminal. Go to (Ctrl-G)
  lists bookmarks, desktop places and frecency history.
- **Commands and appearance:** Ctrl-P command palette, F1 help, a
  configurable keymap. F2 tools (TOML, built-in and Lua plugins) and openers
  (Shift-Enter). A theme picker with live preview.
- **Settings:** TOML in `<config dir>/fyle/config.toml`. The test checks
  that `config.example.toml` equals the defaults, so update both together.
- **CI:** `.github/workflows/ci.yml` runs build and test on ubuntu-latest
  only.

## Code map (`src/`)

| Module | Role |
|---|---|
| `main.rs` | Startup, settings, key bindings, window |
| `commander.rs` | Main window. Owns the two panels and all actions, dialogs, jobs bar, tools orchestration. It is large (~1600 lines); split it if you add much more. |
| `model.rs` | `Folder`: entries, cursor, marks, filter, sort, tree, search results |
| `panel.rs` | `FolderView` (DataTable delegate `FileTable`) and the filter bar |
| `fs_ops.rs` | `Entry`, `Attrs`, read_dir, copy/move/delete/trash, formatting |
| `columns.rs` | `ColumnKind`. A new attribute column means a variant here plus a renderer in `panel.rs`. |
| `preview/` | `Previewer` trait and `Registry`. Order: directory, image, pdf, media, archive, markdown, table, text, binary. `preview_view.rs` has one render arm per `Content` variant. |
| `editor.rs` / `editor_view.rs` | `Document` model (UTF-8, ≤ 8 MiB, atomic save) and the editor pane |
| `tools/` | Tool, Action and Registry model; `defaults.toml`; built-ins (git website, just tasks, image gallery); `exec.rs` runs them |
| `search.rs`, `jobs.rs`, `places.rs`, `rename.rs`, `openers.rs`, `palette.rs`, `themes.rs`, `icons.rs`, `settings.rs` | UI-independent models, each with tests |
| `*_view.rs`, `theme_picker.rs` | Thin views |

## Gotchas learned the hard way

- Key contexts:
  - `Commander` holds global keys.
  - `Commander > DataTable` holds panel keys. Space and Backspace must not
    fire inside the editor, which is why they live here.
  - `EditorPane` and `FilterBar` hold their own keys.
- Dialogs restore focus 250 ms after they close. To focus something
  afterwards (e.g. the editor after the new-file dialog), spawn a task
  with a 300 ms timer.
- Never write `use super::*` in a test module of a file that glob-imports
  `gpui_kit::*`. It pulls in gpui's `test` macro and breaks `#[test]`.
  Import items explicitly.
- `preview::Registry` is imported as `PreviewRegistry` in `commander.rs`,
  because the name clashes with the tools Registry.
- The lockfile pins `cc` to 1.2.67, because tree-sitter-sequel needs it.
  Don't `cargo update` it blindly.
- Disk: `[profile.dev.package."*"] debug = false` keeps `target/` at about
  3 GB. Without it the target grew to 29 GB and filled the container. Run
  `cargo clean` if space runs out.

## How to verify

- Before every commit run `cargo fmt --check` and `cargo test`.
  `cargo clippy` currently reports 5 style warnings (`question_mark`,
  `while_let_on_iterator`, `type_complexity`) and none are fixed yet; CI
  doesn't run clippy.
- UI checks run headless:
  - Needs `xvfb`, `xdotool`, `imagemagick`, `libxkbcommon-x11-dev` and
    `mesa-vulkan-drivers` (lavapipe; there's no GPU).
  - Run the app as:

    ```sh
    Xvfb :99 -screen 0 1280x800x24 &
    export DISPLAY=:99
    XDG_CONFIG_HOME=$SCRATCH/cfg XDG_DATA_HOME=$SCRATCH/data ./target/debug/fyle DIR1 DIR2 &
    W=$(xdotool search --name Fyle | tail -1)
    xdotool key --window $W F3
    import -window root shot.png
    ```

  - `xdotool type` is too fast for text areas; send one `xdotool key` per
    character.
  - Never `pkill -f` a pattern that also matches your own shell.
- Always point `XDG_CONFIG_HOME` and `XDG_DATA_HOME` at a scratch
  directory, so the real config and history stay untouched.

## Next steps (suggested order)

1. **Tabs per panel:** parked by the owner; ask before starting.
2. **Shell command on the selection** with output capture: the owner found
   it unclear. Only "open terminal here" (F9) was wanted, and it is done.
3. **More previews:** PDF pages beyond the first.
4. **EXIF column** (the preview shows EXIF already).

Dropped by the owner: Zed theme import and icon themes.

## Open decisions (ask the owner)

- **Embedded terminal.** The recommendation was: none for now (F9 opens an
  external terminal). Later, if wanted, use `alacritty_terminal` plus a PTY
  rendered in gpui, like Zed (GPL-compatible).
- **Release size** is settled: under 100 MB was a wish, not a rule. The
  release profile (strip, LTO, one codegen unit) gives 75 MB, 19 MB
  gzipped, with every grammar kept.
