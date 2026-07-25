#!/bin/sh
set -eu

printf '%s\n' "$@" > "$0.arguments"
printf 'ADDED PWF TASK [PWF-0001] test-project :: title\n'
