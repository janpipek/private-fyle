# TODO

- [ ] More preview types (pluggable via `preview::Previewer`): PDF, archives
      (list contents), audio/video metadata, Markdown rendered, CSV as table,
      EXIF next to images.
- [ ] Editor: optional external editor (`$EDITOR` / setting), Shift-F4 new file, warn when the file changed on disk.
- [ ] Icon themes: loadable icon sets (e.g. Zed icon themes), optional per-type colours.
- [ ] Tools UI: menu of offers (grouped, disabled when a program is missing), parameter form, pick list, log view, confirmation; load the user's tools.toml. Model is done (`src/tools/`, `docs/tools.md`).
- [ ] Find files (fd-like) and search contents (rg-like): results as a virtual panel
      (navigate, preview, copy/move from it), streamed while searching, cancellable.
      Built on the `ignore`/`grep` crates (ripgrep's own libraries), no
      external `fd`/`rg` needed; works the same everywhere, incl. Windows.
- [ ] Ideas from yazi (terminal file manager):
  - [ ] Background task manager: copy/move/delete run async with progress,
        cancel, and a task list; panels stay responsive.
  - [ ] Yank/cut/paste between panels and tabs, besides F5/F6.
  - [ ] Tabs per panel.
  - [ ] Bulk rename of the selection in the editor (one name per line).
  - [ ] Openers: rules by MIME type/extension choosing which app opens a file
        (incl. "open with…" list).
  - [ ] Richer previews: PDF, video thumbnails, archive listing, scrolling
        and paging inside the preview.
  - [ ] Linemode: choose what the extra columns show (size, mtime,
        permissions, owner) — fits the attribute-columns item above.
  - [ ] Quick jump: zoxide-style frecency of visited directories, bookmarks.
  - [ ] Shell command on selection with output capture (ties into tools).
  - [ ] Configurable keymap and a help overlay listing bindings.
  - [ ] Lua plugins via `mlua` (features `lua54`, `vendored`: Lua compiled in,
        nothing to install; prototype builds for Linux and Windows). A plugin
        defines tools (`id`, `group`, `label`, `applies(ctx)`, `params`,
        `plan(ctx)` returning an action table) adapted to the `Tool` trait;
        plugins live in `<config dir>/fyle/plugins/*.lua`. Same trust rule as
        tools.toml: only from the user's config.
- [ ] Zed theme import (converter from Zed's theme JSON).
- [ ] Favourite locations from the desktop environment
      (Windows Quick Access / known folders, KDE Places `user-places.xbel`,
      GTK bookmarks `~/.config/gtk-3.0/bookmarks`, XDG user dirs).
- [ ] Extensible attribute columns (open-ended): pluggable per-entry
      attributes shown as optional columns, e.g. EXIF data, SELinux context,
      git status, permissions/owner, xattrs.
- [ ] macOS support (later): CI job, app menu, Cmd shortcuts, app bundle.

## Done

- [x] Ctrl-P command palette with key bindings (palette.rs model, palette_view.rs).
- [x] Filter as you type (/ or Ctrl-F), kept in the Folder model, cleared on directory change.
- [x] F8 moves to the system trash (trash crate; Recycle Bin on Windows); Shift-F8 deletes permanently.
- [x] Natural sort (file2 < file10), case-insensitive.
- [x] F4 built-in editor (syntax highlighting, atomic save, unsaved-changes prompt).
- [x] File-type icons, mapped by name/extension in `icons.rs`; glyphs from GPUI Kit's default bundle.
- [x] Runtime theme picker with live preview (Ctrl-K Ctrl-T).
- [x] Themes: 36 bundled (GPUI Kit), user theme folder, `theme` setting.
- [x] F3 quick preview: text with syntax highlighting, images, directories, hex dump.
- [x] Tree mode (toggleable per panel, Ctrl-T).
