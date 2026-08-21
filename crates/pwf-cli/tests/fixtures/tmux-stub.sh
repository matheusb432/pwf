#!/usr/bin/env bash
set -eu

if [ -n "${TMUX_STUB_LOG:-}" ]; then
  printf '%s\000' "$*" >>"$TMUX_STUB_LOG"
fi

case "${1:-}" in
  -V)
    echo "tmux 3.4"
    ;;
  has-session)
    if [ "${TMUX_STUB_SESSION_EXISTS:-1}" = "1" ]; then
      exit 0
    fi
    printf '%s\n' "can't find session: pwf" >&2
    exit 1
    ;;
  new-window)
    exit "${TMUX_STUB_NEW_WINDOW_EXIT_CODE:-0}"
    ;;
  new-session)
    exit 97
    ;;
esac
