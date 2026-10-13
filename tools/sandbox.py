#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["click>=8.1"]
# ///
"""Build and run the fyle development sandbox container.

A rootless-Podman (or Docker) container, Fedora-44 to match the host, in which
an AI coding agent can build, test and edit this repo autonomously: the working
tree is bind-mounted read-write at its identical absolute path, files stay
host-owned (--userns=keep-id), and the container is the safety boundary so the
agent runs with permissions skipped.

Headless by default (cargo build + test). Pass --gui to wire up Wayland + the
GPU render nodes so the gpui window can open, and --mount DIR (repeatable) to
mount extra directories read-only for browsing in the file manager.

\b
Examples:
    tools/sandbox.py build              # build the image (cached)
    tools/sandbox.py run                # plain bash shell (default)
    tools/sandbox.py run --claude       # autonomous Claude (skips permissions)
    tools/sandbox.py run --codex        # interactive Codex
    tools/sandbox.py run --pi           # interactive Pi
    tools/sandbox.py run --gui --mount ~/Pictures
    tools/sandbox.py run --exec 'cargo test'  # one-shot command, then exit
"""

from __future__ import annotations

import os
import shlex
import shutil
import subprocess
import sys
from pathlib import Path

import click

REPO = Path(__file__).resolve().parent.parent
IMAGE = "fyle-dev"
CONTAINERFILE_DIR = REPO / ".devcontainer"

# Agent commands; the container is the boundary, so they run unattended.
AGENTS = {
    "claude": ["claude", "--dangerously-skip-permissions"],
    # --yolo alone only sets the start-up permissions; also make full access the
    # configured default, so a mid-session settings change is less likely to land
    # on a bwrap profile (bwrap cannot mount /proc inside the container).
    "codex": [
        "codex", "--yolo",
        "-c", 'sandbox_mode="danger-full-access"',
        "-c", 'approval_policy="never"',
    ],
    # Pi has no permission prompts to skip.
    "pi": ["pi"],
}


def backend() -> str:
    """Container CLI: honour CONTAINER_BACKEND, else prefer podman, else docker."""
    chosen = os.environ.get("CONTAINER_BACKEND")
    candidates = [chosen] if chosen else ["podman", "docker"]
    for name in candidates:
        if name and shutil.which(name):
            return name
    raise click.ClickException(
        "no container backend found (install podman or docker, or set CONTAINER_BACKEND)"
    )


def image_exists(cli: str) -> bool:
    return subprocess.run([cli, "image", "exists", IMAGE]).returncode == 0


def build_image(cli: str) -> int:
    """Build the sandbox image from .devcontainer/Containerfile."""
    cmd = [
        cli,
        "build",
        "-t",
        IMAGE,
        "-f",
        str(CONTAINERFILE_DIR / "Containerfile"),
        str(CONTAINERFILE_DIR),
    ]
    click.echo("+ " + " ".join(cmd))
    return subprocess.run(cmd).returncode


def gui_args() -> list[str]:
    """Wayland socket + GPU render nodes so gpui can open a window (Vulkan)."""
    runtime = os.environ.get("XDG_RUNTIME_DIR")
    if not runtime:
        raise click.UsageError(
            "--gui needs XDG_RUNTIME_DIR set (are you in a graphical session?)"
        )
    wayland = os.environ.get("WAYLAND_DISPLAY", "wayland-0")
    socket = Path(runtime) / wayland
    if not socket.exists():
        raise click.UsageError(
            f"--gui needs a Wayland socket at {socket} (this path expects a Wayland session)"
        )
    return [
        # Mesa's Vulkan driver uses the integrated GPU; the NVIDIA card is
        # intentionally not wired up. keep-groups preserves the host
        # 'render'/'video' groups for the device nodes.
        "--device", "/dev/dri",
        "--group-add", "keep-groups",
        "-v", f"{socket}:{socket}",
        "-e", f"XDG_RUNTIME_DIR={runtime}",
        "-e", f"WAYLAND_DISPLAY={wayland}",
    ]  # fmt: skip


@click.group(invoke_without_command=True, help=__doc__)
@click.pass_context
def cli(ctx: click.Context) -> None:
    ctx.obj = backend()
    # Default to `run` with its defaults when no subcommand is given.
    if ctx.invoked_subcommand is None:
        ctx.invoke(run)


@cli.command()
@click.pass_obj
def build(container_cli: str) -> None:
    """Build the sandbox container image."""
    sys.exit(build_image(container_cli))


@cli.command()
@click.option("--gui", is_flag=True, help="Wire up Wayland + GPU so the gpui window can open.")
@click.option(
    "--mount",
    "mounts",
    metavar="DIR",
    multiple=True,
    type=click.Path(exists=True, file_okay=False, resolve_path=True, path_type=Path),
    help="Mount a directory read-only (repeatable).",
)
@click.option("--claude", "agents", flag_value="claude", multiple=True, help="Launch autonomous Claude (--dangerously-skip-permissions) instead of a shell.")
@click.option("--codex", "agents", flag_value="codex", multiple=True, help="Launch interactive Codex (--yolo) instead of a shell.")
@click.option("--pi", "agents", flag_value="pi", multiple=True, help="Launch interactive Pi instead of a shell.")
@click.option("--dry-run", is_flag=True, help="Print the container command without running it.")
@click.option("--exec", "command", metavar="CMD", help="Run a one-shot shell-quoted command instead of a shell, then exit.")
@click.pass_obj
def run(
    container_cli: str,
    gui: bool = False,
    mounts: tuple[Path, ...] = (),
    agents: tuple[str, ...] = (),
    dry_run: bool = False,
    command: str | None = None,
) -> None:
    """Run the sandbox (default)."""
    if len(agents) + bool(command) > 1:
        raise click.UsageError("--claude, --codex, --pi and --exec are mutually exclusive")

    if not dry_run and not image_exists(container_cli):
        click.echo(f"image '{IMAGE}' not found; building it first...")
        rc = build_image(container_cli)
        if rc != 0:
            sys.exit(rc)

    cmd = [
        container_cli, "run", "--rm", "-it",
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
        # Same for Pi (~/.pi/agent holds its auth and settings).
        "-v", "fyle-pi:/home/dev/.pi",
        "--hostname", "fyle-sandbox",
    ]  # fmt: skip

    if gui:
        cmd += gui_args()

    for path in mounts:
        cmd += ["-v", f"{path}:{path}:ro"]

    cmd.append(IMAGE)

    if command:
        cmd += shlex.split(command)
    elif agents:
        cmd += AGENTS[agents[0]]
    else:
        cmd += ["bash"]

    click.echo("+ " + " ".join(cmd))
    if dry_run:
        return
    sys.exit(subprocess.run(cmd).returncode)


if __name__ == "__main__":
    cli()
