---
title: "Getting started"
weight: 10
---

The private-fyle project builds the app named **fyle**. Fyle opens two folder panels. The left panel starts in the current directory;
the right starts in your home directory unless you provide paths or change the
settings. Press **Tab** to switch panels, use the arrow keys to move, and press
**Enter** to open a folder or file. Press **F1** for the live command list or
**Ctrl-P** to search all commands.

The usual workflow is to select entries in one panel, then copy with **F5** or
move with **F6** to the folder shown in the other panel. Fyle asks how to handle
existing destination files and shows background jobs with progress and cancel
controls. **F3** opens the inspector preview; **Alt-Enter** shows Properties.

## Run from source

From a checkout of private-fyle, build and start the app with Rust's Cargo:

```sh
cargo run --release -- ~/Downloads ~/Documents
```

The two paths are optional. Without them, fyle opens the current directory on
the left and your home directory on the right. Linux build dependencies and
package commands are listed in the [project README](https://github.com/janpipek/private-fyle/blob/main/README.md#build-dependencies).

Once installed, start in specific folders by passing them as arguments:

```sh
fyle ~/Downloads ~/Documents
```

For remote setup, see [Remote storage]({{< relref "remotes.md" >}}). To change startup folders,
views, columns or shortcuts, see [Settings]({{< relref "settings.md" >}}).

Fyle currently targets Linux. Windows is paused and macOS support is planned
later.
