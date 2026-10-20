# Development

See [AGENTS.md](../AGENTS.md) for coding rules, verification commands and
UI testing instructions.

## Build dependencies

- Linux (Debian/Ubuntu): `libxkbcommon-dev libxkbcommon-x11-dev libfontconfig1-dev libxcb1-dev libwayland-dev libssl-dev pkg-config`,
  plus a Vulkan driver at runtime.
- Windows: MSVC toolchain.

The Rust CI job is currently disabled because compilation can exhaust runner
disk space; see [TODO](../TODO.md). The zizmor workflow audit remains enabled
in `.github/workflows/ci.yml`.

## Packages

`just rpm` builds a release binary on the host and packages it as a Fedora
RPM in `dist/fedora` (needs `rpm-build` and `desktop-file-utils`). Its
desktop entry offers fyle for opening folders. It is an interim setup: no
source RPM yet.

`just deb` does the same for Debian and Ubuntu: it packages the host-built
binary as a `.deb` in `dist/debian` with `dpkg-deb` (needs `dpkg-dev` and
`desktop-file-utils`). Dependencies come from `dpkg-shlibdeps`, so the
package suits the system it was built on.

On Windows, `just installer` builds an x64 release binary and an Inno Setup
installer in `dist/windows` (requires `iscc.exe`). Setup defaults to a
current-user install and also offers an all-users install. It creates a Start
menu shortcut and adds **Open in fyle** for folders, drive roots, and folder
backgrounds in Explorer. An unchecked setup option lets the user make fyle the
default folder opener; uninstall restores the previous folder verb if fyle is
still the default. No desktop shortcut is created. The installer is unsigned
and uses the current executable icon until an app icon is supplied.

## Development sandbox

A Fedora 44 container (rootless Podman, or Docker) with the Rust toolchain,
the build dependencies, Claude Code and Codex CLI, so an agent can work on the repo
autonomously with the container as the safety boundary. The repo is
bind-mounted read-write at its host path (sharing `target/`), and the
container runs as your UID, so files stay yours.

```sh
just sandbox-build              # build the image (also done on first run)
just sandbox                    # bash shell
just sandbox --claude           # Claude Code with permissions skipped
just sandbox --codex            # interactive Codex (default permissions)
just sandbox --gui --mount ~/   # Wayland + GPU, extra dirs read-only
just sandbox --exec 'cargo test'  # one-shot command
```

`.devcontainer/` uses the same image for VS Code, JetBrains and Zed.

After adding Codex to an existing setup, run `just sandbox-build` (or rebuild
your IDE's dev container). Sign in once with
`just sandbox --exec 'codex login --device-auth'`, then run `just sandbox --codex`.
Follow the browser link and enter the code; device code login must be enabled
in your ChatGPT security settings or workspace permissions
([OpenAI authentication docs](https://developers.openai.com/codex/auth#login-on-headless-devices)).
The `fyle-codex` named volume keeps Codex credentials, configuration and sessions
across container exits and rebuilds, and is shared by the CLI launcher and IDE
dev container.
