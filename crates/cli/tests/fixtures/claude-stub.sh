#!/bin/sh
# Records the repository and argument boundaries of one native Claude launch.
set -u

case "${1:-}" in
  --version) printf 'claude fixture 1.0\n'; exit 0 ;;
esac

log_path=${CLAUDE_STUB_LOG:?CLAUDE_STUB_LOG is required}
{
  printf 'cwd=%s\000' "$PWD"
  for argument in "$@"; do
    printf 'arg=%s\000' "$argument"
  done
} >"$log_path"

exit "${CLAUDE_STUB_EXIT_CODE:-0}"
