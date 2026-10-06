# List available tasks
default:
    @just --list

# Run the application
run:
    cargo run

# Build the development sandbox container image
sandbox-build:
    uv run tools/sandbox.py build

# Run the dev sandbox (bash shell); pass --claude or --codex, plus --gui, --mount DIR
sandbox *args:
    uv run tools/sandbox.py run {{args}}
