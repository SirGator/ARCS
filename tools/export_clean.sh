#!/usr/bin/env bash

set -euo pipefail

if ! git rev-parse --show-toplevel >/dev/null 2>&1; then
    printf 'error: export_clean.sh must be run inside a git repository\n' >&2
    exit 1
fi

repo_root="$(git rev-parse --show-toplevel)"
output_path="${1:-ARCS-clean.zip}"

if [[ -n "$(git -C "$repo_root" status --porcelain)" ]]; then
    printf 'error: export_clean.sh requires a clean working tree\n' >&2
    printf 'hint: commit or stash changes before exporting\n' >&2
    exit 1
fi

git -C "$repo_root" archive --format=zip --output "$output_path" HEAD
printf 'created clean export: %s\n' "$output_path"
