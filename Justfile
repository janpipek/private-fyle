# List available tasks
default:
    @just --list

# Run the application
run:
    cargo run

# Build the development sandbox container image
sandbox-build:
    uv run tools/sandbox.py build

# Run codex agent in a sandbox
codex:
    just sandbox --codex

# Run claude agent in a sandbox
claude:
    just sandbox --claude

# Run pi agent in a sandbox
pi:
    just sandbox --pi

# Run the dev sandbox (bash shell); pass --claude, --codex, --pi or --exec CMD, plus --gui, --mount DIR
[positional-arguments]
sandbox *args:
    uv run tools/sandbox.py run "$@"

# Build a release binary and package it as a Fedora RPM in dist/fedora
[linux]
rpm:
    bash tools/package_fedora.sh

# Build a Windows release binary and Inno Setup installer in dist/windows
[windows]
installer:
    powershell -NoProfile -ExecutionPolicy Bypass -File tools/package_windows.ps1
