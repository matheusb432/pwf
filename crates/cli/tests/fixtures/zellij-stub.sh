#!/bin/sh
# Recording zellij stub for the session e2e (pattern of pw-stub.sh): appends its
# argv to $ZELLIJ_STUB_LOG with NUL invocation boundaries. Stateful
# missing-session modes accept one or two tab failures.
set -u

case "${1:-}" in
  --version)
    if [ -n "${ZELLIJ_STUB_LOG_VERSION:-}" ] && [ -n "${ZELLIJ_STUB_LOG:-}" ]; then
      printf '%s\n' "--version" >>"$ZELLIJ_STUB_LOG"
    fi
    echo "zellij 0.44.3"
    exit 0
    ;;
esac

if [ -n "${ZELLIJ_STUB_LOG:-}" ]; then
  printf '%s\000' "$*" >>"$ZELLIJ_STUB_LOG"
fi

missing_session_count=${ZELLIJ_STUB_MISSING_SESSION_COUNT:-0}
case "$missing_session_count" in
  0) ;;
  1 | 2)
    if [ "${4:-}" = "new-tab" ]; then
      state_path=${ZELLIJ_STUB_STATE:?ZELLIJ_STUB_STATE is required}
      attempt=0
      if [ -f "$state_path" ]; then
        IFS= read -r attempt <"$state_path"
      fi
      case "$attempt" in
        0 | 1) ;;
        *) printf 'unexpected new-tab attempt: %s\n' "$attempt" >&2; exit 97 ;;
      esac
      attempt=$((attempt + 1))
      printf '%s\n' "$attempt" >"$state_path"
      if [ "$attempt" -le "$missing_session_count" ]; then
        printf "Session '%s' not found\n" "${2:-}" >&2
        exit 1
      fi
    fi
    ;;
  *) printf 'invalid missing session count: %s\n' "$missing_session_count" >&2; exit 97 ;;
esac

if [ -n "${ZELLIJ_STUB_STDERR:-}" ]; then
  printf '%s\n' "$ZELLIJ_STUB_STDERR" >&2
fi
exit "${ZELLIJ_STUB_EXIT_CODE:-0}"
