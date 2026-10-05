#!/usr/bin/env python3
"""Build and run the fyle development sandbox container.

A rootless-Podman (or Docker) container, Fedora-44 to match the host, in which
an AI coding agent can build, test and edit this repo autonomously: the working
tree is bind-mounted read-write at its identical absolute path, files stay
host-owned (--userns=keep-id), and the container is the safety boundary so the
agent runs with permissions skipped.

Headless by default (cargo build + test). Pass --gui to wire up Wayland + the
GPU render nodes so the gpui window can open, and --mount DIR (repeatable) to
mount extra directories read-only for browsing in the file manager.

Examples:
    python tools/sandbox.py build              # build the image (cached)
    python tools/sandbox.py run                # plain bash shell (default)
    python tools/sandbox.py run --claude       # autonomous Claude (skips permissions)
    python tools/sandbox.py run --codex        # interactive Codex
    python tools/sandbox.py run --gui --mount ~/Pictures
    python tools/sandbox.py run -- cargo test # one-shot command, then exit
"""
from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
IMAGE = "fyle-dev"
CONTAINERFILE_DIR = REPO / ".devcontainer"


def backend() -> str:
    """Container CLI: honour CONTAINER_BACKEND, else prefer podman, else docker."""
    chosen = os.environ.get("CONTAINER_BACKEND")
    candidates = [chosen] if chosen else ["podman", "docker"]
    for name in candidates:
        if name and shutil.which(name):
            return name
    sys.exit("error: no container backend found (install podman or docker, or set CONTAINER_BACKEND)")


def image_exists(cli: str) -> bool:
    return subprocess.run([cli, "image", "exists", IMAGE]).returncode == 0


def build(cli: str) -> int:
    """Build the sandbox image from .devcontainer/Containerfile."""
    cmd = [cli, "build", "-t", IMAGE, "-f", str(CONTAINERFILE_DIR / "Containerfile"), str(CONTAINERFILE_DIR)]
    print("+", " ".join(cmd))
    return subprocess.run(cmd).returncode


def gui_args() -> list[str]:
    """Wayland socket + GPU render nodes so gpui can open a window (Vulkan)."""
    runtime = os.environ.get("XDG_RUNTIME_DIR")
    if not runtime:
        sys.exit("error: --gui needs XDG_RUNTIME_DIR set (are you in a graphical session?)")
    wayland = os.environ.get("WAYLAND_DISPLAY", "wayland-0")
    socket = Path(runtime) / wayland
    if not socket.exists():
        sys.exit(f"error: --gui needs a Wayland socket at {socket} (this path expects a Wayland session)")
    return [
        # Mesa's Vulkan driver uses the integrated GPU; the NVIDIA card is
        # intentionally not wired up. keep-groups preserves the host
        # 'render'/'video' groups for the device nodes.
        "--device", "/dev/dri",
        "--group-add", "keep-groups",
        "-v", f"{socket}:{socket}",
        "-e", f"XDG_RUNTIME_DIR={runtime}",
        "-e", f"WAYLAND_DISPLAY={wayland}",
    ]


def run(cli: str, args: argparse.Namespace) -> int:
    if not args.dry_run and not image_exists(cli):
        print(f"image '{IMAGE}' not found; building it first...")
        rc = build(cli)
        if rc != 0:
            return rc

    cmd = [
        cli, "run", "--rm", "-it",
        # Run as the host user so files written to the bind mount stay editable.
        "--userns=keep-id",
        # Access the bind mounts under enforcing SELinux without recursively
        # relabelling host directories.
        "--security-opt", "label=disable",
        # Identical absolute path lets host and container share target/.
        "-v", f"{REPO}:{REPO}",
        "-w", str(REPO),
        "-e", "HOME=/home/dev",
        # Keep Codex login/config across ephemeral containers and IDE sessions.
        "-v", "fyle-codex:/home/dev/.codex",
        "--hostname", "fyle-sandbox",
    ]

    if args.gui:
        cmd += gui_args()

    for mount in args.mount:
        path = Path(mount).expanduser().resolve()
        if not path.is_dir():
            sys.exit(f"error: --mount path is not a directory: {path}")
        cmd += ["-v", f"{path}:{path}:ro"]

    cmd.append(IMAGE)

    if args.command:
        cmd += args.command
    elif args.claude:
        # The container is the boundary, so the agent runs unattended.
        cmd += ["claude", "--dangerously-skip-permissions"]
    elif args.codex:
        cmd += ["codex"]
    else:
        cmd += ["bash"]

    print("+", " ".join(cmd))
    if args.dry_run:
        return 0
    return subprocess.run(cmd).returncode


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command_name")

    sub.add_parser("build", help="build the sandbox container image")

    run_p = sub.add_parser("run", help="run the sandbox (default)")
    run_p.add_argument("--gui", action="store_true", help="wire up Wayland + GPU so the gpui window can open")
    run_p.add_argument("--mount", metavar="DIR", action="append", default=[], help="mount a directory read-only (repeatable)")
    agent = run_p.add_mutually_exclusive_group()
    agent.add_argument("--claude", action="store_true", help="launch autonomous Claude (--dangerously-skip-permissions) instead of a shell")
    agent.add_argument("--codex", action="store_true", help="launch interactive Codex instead of a shell")
    run_p.add_argument("--dry-run", action="store_true", help="print the container command without running it")
    run_p.add_argument("command", nargs=argparse.REMAINDER, help="optional one-shot command (after --)")

    args = parser.parse_args(argv)
    cli = backend()

    if args.command_name == "build":
        return build(cli)

    # Default to `run` with its defaults when no subcommand is given.
    if args.command_name != "run":
        args = run_p.parse_args([])

    # argparse.REMAINDER keeps a leading "--"; drop it so exec gets a clean argv.
    if args.command and args.command[0] == "--":
        args.command = args.command[1:]
    return run(cli, args)


if __name__ == "__main__":
    raise SystemExit(main())
