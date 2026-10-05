# List available tasks
default:
    @just --list

# Build the development sandbox container image
sandbox-build:
    uv run tools/sandbox.py build

# Run the dev sandbox (bash shell); pass --claude for the agent, plus --gui, --mount DIR
sandbox *args:
    uv run tools/sandbox.py run {{args}}
