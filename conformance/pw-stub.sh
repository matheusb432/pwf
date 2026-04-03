#!/bin/sh
# Conformance stub for the handoff `--pending-work-script` seam: a minimal external
# pending-work allocator that speaks pwf's canonical CLI protocol
# (`<verb> --config-path … --id … --json --date … <words…>`). `add` emits a fixed
# id as JSON; `check` is a no-op. Both append to $HANDOFF_STUB_LOG when set.
set -u

verb=${1:-}
[ $# -gt 0 ] && shift

id=
words=
while [ $# -gt 0 ]; do
  case $1 in
    --id) shift; id=${1:-} ;;
    --config-path|--date) shift ;; # consume + ignore the value
    --json) : ;;
    --*) : ;;                       # ignore any other flag
    *) words="${words:+$words }$1" ;;
  esac
  shift
done

case $verb in
  add)
    if [ -n "${HANDOFF_STUB_LOG:-}" ]; then printf 'add %s\n' "$words" >>"$HANDOFF_STUB_LOG"; fi
    printf '{"id":"TST-0001"}\n'
    ;;
  check)
    if [ -n "${HANDOFF_STUB_LOG:-}" ]; then printf 'check %s\n' "$id" >>"$HANDOFF_STUB_LOG"; fi
    ;;
esac
