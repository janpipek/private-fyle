# fyle

A two-panel file manager in the spirit of Krusader / Midnight Commander,
built in Rust with [GPUI Kit](https://gpui-kit.com). Linux first; Windows
work is paused and macOS comes later. GPL-3.0-or-later.

Browse local folders, archives and remote storage; copy and move with
background jobs; search, preview and edit files; customize views, shortcuts
and context-aware tools.

```sh
cargo run --release -- [LEFT_DIR] [RIGHT_DIR]
```

Paths are optional: left defaults to the current directory, right to home.
F1 lists commands; Ctrl-P opens the command palette.

- [User guide](site/content/_index.md): [getting started](site/content/getting-started.md),
  [shortcuts](site/content/shortcuts.md), [settings](site/content/settings.md),
  [remote storage](site/content/remotes.md) and [tools](site/content/tools.md).
- [Google Drive setup](HOWTO.md).
- [Build, packages and development sandbox](docs/development.md).
- [Architecture and module map](docs/architecture.md),
  [migration record](docs/architecture-plan.md) and
  [navigation lifecycle](docs/navigation-lifecycle-plan.md).
- [Detailed behavior and limits](docs/behavior.md).
- [Tool execution contracts](docs/tools.md) and [reader extensions](docs/extensions.md).
- [TODO](TODO.md): open work, decisions and completed features.
- [AGENTS.md](AGENTS.md): contributor rules and required checks.
- [Guide development](site/README.md): preview with `just docs` (requires Hugo).

See [LICENSE](LICENSE) for GPL-3.0-or-later terms and
[theme attribution](themes/README.md) for bundled GPUI Kit themes.
