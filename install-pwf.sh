#!/usr/bin/env bash
# Linux bootstrap for the global `pwf` shim (the analog of the Windows scoop manifest).
# Thin delegator: the build + symlink + PATH logic lives once in `just update` (Linux
# branch). Kept as a stable path-based entry point because repository provisioning
# (`just install-pwf` → agent-tooling-linux.ps1) invokes it here; `just` is always present
# at that point (provisioning calls it via `just`).
set -euo pipefail
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec just --justfile "$repo/justfile" --working-directory "$repo" update
