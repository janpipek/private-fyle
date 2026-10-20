---
title: "Getting started"
weight: 10
---

The private-fyle project builds the app named **fyle**. Fyle opens two folder
panels. The left panel starts in the current directory; the right starts in
your home directory unless you provide paths or change the settings.

- {{< key "Tab" >}} switches panels.
- The arrow keys move the cursor.
- {{< key "Enter" >}} opens a folder or file.
- {{< key "F1" >}} shows the live command list.
- {{< key "Ctrl" "P" >}} searches all commands.

The usual workflow is to select entries in one panel, then copy with {{< key "F5" >}} or
move with {{< key "F6" >}} to the folder shown in the other panel. Fyle asks how to handle
existing destination files and shows background jobs with progress and cancel
controls. {{< key "F3" >}} opens the inspector preview; {{< key "Alt" "Enter" >}} shows Properties.

{{< shot src="images/fyle-panels.png" alt="Two folder panels labeled Source and Destination" caption="Keep the source in one panel and the destination in the other." >}}

## Run from source

From a checkout of private-fyle, build and start the app with Rust's Cargo:

```sh
cargo run --release -- ~/Downloads ~/Documents
```

The two paths are optional. Without them, fyle opens the current directory on
the left and your home directory on the right. Linux build dependencies and
package commands are listed in the [development guide](https://github.com/janpipek/private-fyle/blob/main/docs/development.md#build-dependencies).

Once installed, start in specific folders by passing them as arguments:

```sh
fyle ~/Downloads ~/Documents
```

For remote setup, see [Remote storage]({{< relref "remotes.md" >}}). To change startup folders,
views, columns or shortcuts, see [Settings]({{< relref "settings.md" >}}).

Fyle currently targets Linux. Windows is paused and macOS support is planned
later.
