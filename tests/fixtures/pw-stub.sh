#!/bin/sh
# Minimal pending-work stub for handoff tests that exercise --pending-work-script.
set -u

verb=${1:-}
[ $# -gt 0 ] && shift

id=
words=
while [ $# -gt 0 ]; do
  case $1 in
    --id) shift; id=${1:-} ;;
    --config-path|--date) shift ;;
    --json) : ;;
    --*) : ;;
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
