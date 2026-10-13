#!/bin/sh
set -eu

codex_home=${CODEX_HOME:-$HOME/.codex}
if [ ! -d "$codex_home/plugins/cache/mattpocock/mattpocock-skills" ]; then
    if [ ! -d "$codex_home/.tmp/marketplaces/mattpocock" ]; then
        codex plugin marketplace add mattpocock/skills
    fi
    codex plugin add mattpocock-skills@mattpocock
fi

exec "$@"
