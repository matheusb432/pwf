#!/bin/sh
# Minimal pending-work stub for handoff tests that exercise --pending-work-script.
# Handoff allocation only invokes `add`.
set -u

if [ -n "${HANDOFF_STUB_LOG:-}" ]; then
  : >"$HANDOFF_STUB_LOG"
  for argument in "$@"; do
    printf '%s\n' "$argument" >>"$HANDOFF_STUB_LOG"
  done
fi

verb=${1:-}

case $verb in
  add)
    printf 'ADDED PWF TASK [TST-0001] test-project :: continue managed flow\n'
    ;;
esac
