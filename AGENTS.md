# AGENTS.md

Instructions for coding agents working on fyle. `README.md` covers usage,
settings and architecture (with the module map); `TODO.md` is the backlog
and the place for open decisions; `docs/tools.md` covers the tools design.

## Project

A simple two-panel file manager in the spirit of Krusader, in Rust on
GPUI Kit (`gpui-kit` 0.7.0: gpui, gpui-component and default assets).
GPL-3.0-or-later. Linux first; Windows is paused, macOS comes later.

## Working with the owner

- Keep answers short, precise and honest. Say when you are unsure.
- Ask before starting anything parked or undecided in `TODO.md`.
- One feature per commit, with a concise imperative subject. Commit; never
  push and never open a PR unless asked.
- Others may edit the tree at the same time. Stage only your own changes.
- When you finish a TODO item, move it to "Done" in `TODO.md` and update
  `README.md`.

## Code rules

- Model first: every feature gets a UI-independent model with unit tests,
  and a thin view on top. Models live in their own modules (`model.rs`,
  `transfer.rs`, `zoom.rs`, …); `*_view.rs` files only render and forward.
- Keep files under about 1000 lines; split by responsibility when one
  grows past that. `commander.rs` is far over: add only wiring there and
  put new logic in new modules.
- Every user-visible command is a `Cmd` in `menus.rs` (label, palette
  label, availability with an explanation, toggle state) mapped to an
  action in `commander::action_for`; menus, the palette and F1 help are all
  built from it.
- Settings: every key in `config.example.toml` must equal the defaults (a
  test checks it). Saving a setting edits the file with `toml_edit`,
  keeping comments and other keys.
- Search uses ripgrep's libraries (`ignore`, `grep-*`), never the `rg` or
  `fd` binaries.
- Don't vendor big asset sets. Icons come only from GPUI Kit's default
  bundle; bundled themes are from gpui-kit (attributed in
  `themes/README.md`). Don't add bundled themes; users add their own to
  the themes folder in their config directory.
- Windows: keep existing `#[cfg(windows)]` code compiling, don't extend it.
- Lua plugins use `mlua` with `lua54` and `vendored`.
- Don't add dependencies without saying why in your reply.

## Rust conventions

- Edition 2024 and default `rustfmt`. Module folders use `foo.rs` next to
  `foo/`, never `foo/mod.rs`.
- Errors: file system and model code returns `io::Result`; at the UI
  boundary errors become `Result<_, String>` messages shaped
  `"<path>: <error>"` for the status line or dialogs. No `anyhow` or
  `thiserror`.
- No `unwrap()` outside tests. Exceptions: `Mutex::lock()`, and invariants
  as `expect("why it cannot fail")`.
- `unsafe` only for libc FFI, each block with a `// SAFETY:` comment.
- Never block the UI thread: file system walks, archive reads, processes
  and anything possibly slow run via `background_spawn`, cancellable when
  they can take long. `render` never touches the disk.
- Every module starts with a `//!` line saying what it is; `pub` items get
  a `///` saying what they do (and why, when not obvious). Comment inline
  only what is not obvious.
- Plain `pub` (one binary crate; no `pub(crate)`).
- Tests: inline `#[cfg(test)] mod tests`; file system tests use
  `tempfile::TempDir`, never the repo or `$HOME`; tests must pass without
  network or optional programs (ffmpeg, poppler).

## Verify

Before every commit:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

CI (`.github/workflows/ci.yml`) runs the same on ubuntu-latest. Fix
warnings rather than `#[allow]`ing them; an `allow` needs a comment why.

Check UI changes in the running app. Headless works with `xvfb`,
`xdotool`, `imagemagick`, `libxkbcommon-x11-dev` and `mesa-vulkan-drivers`:

```sh
Xvfb :99 -screen 0 1280x800x24 &
export DISPLAY=:99
XDG_CONFIG_HOME=$SCRATCH/cfg XDG_DATA_HOME=$SCRATCH/data ./target/debug/fyle DIR1 DIR2 &
W=$(xdotool search --name Fyle | tail -1)
xdotool key --window $W F3
import -window root shot.png
```

- Always point `XDG_CONFIG_HOME` and `XDG_DATA_HOME` at a scratch
  directory so the real config and history stay untouched.
- `xdotool type` is too fast for text fields; send one `xdotool key` per
  character.
- Stop the app with `kill $(pgrep -x fyle)`. Never `pkill -f` a pattern
  that also matches your own shell.

## Gotchas

- Key contexts: `Commander` holds global keys; `Commander > DataTable`
  holds panel keys (Space and Backspace live here so they don't fire in
  the editor); `EditorPane` and `FilterBar` hold their own.
- Dialogs restore focus 250 ms after closing. To focus something
  afterwards (e.g. the editor after the new-file dialog), spawn a task
  with a 300 ms timer.
- Never `use super::*` in a test module of a file that glob-imports
  `gpui_kit::*`: it pulls in gpui's `test` macro and breaks `#[test]`.
  Import items explicitly.
- `preview::Registry` is imported as `PreviewRegistry` in `commander.rs`;
  the name clashes with the tools `Registry`.
- `Cargo.lock` pins `cc` to 1.2.67 for tree-sitter-sequel. Don't
  `cargo update` blindly.
- `[profile.dev.package."*"] debug = false` keeps `target/` near 3 GB
  (it reached 29 GB without it). Run `cargo clean` if the disk fills.
- PDF and media previews need poppler and ffmpeg at runtime; without them
  only parsing is testable.
