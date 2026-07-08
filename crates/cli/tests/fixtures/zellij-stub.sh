#!/bin/sh
# Recording zellij stub for the session e2e (pattern of pw-stub.sh): appends its
# argv to $ZELLIJ_STUB_LOG and exits 0. `--version` answers RealZellij::available()
# without logging; every other call (new-tab) is recorded for the test to assert.
set -u

case "${1:-}" in
  --version) echo "zellij 0.44.3"; exit 0 ;;
esac

if [ -n "${ZELLIJ_STUB_LOG:-}" ]; then
  printf '%s\n' "$*" >>"$ZELLIJ_STUB_LOG"
fi
exit 0
