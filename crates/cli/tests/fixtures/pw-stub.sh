#!/bin/sh
# Minimal pending-work stub for handoff tests that exercise --pending-work-script.
# Handoff allocation only invokes `add`.
set -u

verb=${1:-}
[ $# -gt 0 ] && shift

words=
while [ $# -gt 0 ]; do
  case $1 in
    --config-path|--date) shift ;;
    --*) : ;;
    *) words="${words:+$words }$1" ;;
  esac
  shift
done

case $verb in
  add)
    if [ -n "${HANDOFF_STUB_LOG:-}" ]; then printf 'add %s\n' "$words" >>"$HANDOFF_STUB_LOG"; fi
    printf 'ADDED PWF TASK [TST-0001] test-project :: continue managed flow\n'
    ;;
esac
