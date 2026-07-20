#!/bin/sh
set -eu

arguments_log_path=
argument_previous=
for argument in "$@"; do
    if [ "$argument_previous" = "--config-path" ]; then
        arguments_log_path=$argument
        break
    fi
    argument_previous=$argument
done

if [ -z "$arguments_log_path" ]; then
    printf 'missing --config-path\n' >&2
    exit 64
fi

printf '%s\n' "$@" > "$arguments_log_path"
printf 'ADDED PWF TASK [PWF-0001] test-project :: title\n'
