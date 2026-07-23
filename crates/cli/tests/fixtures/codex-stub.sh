#!/bin/sh
# Implements the app-server and resume boundaries used by Codex session tests.
set -eu

case "${1:-}" in
  --version)
    printf 'codex-cli fixture 1.0\n'
    exit 0
    ;;
  app-server)
    while IFS= read -r line; do
      if [ -n "${CODEX_STUB_APP_SERVER_LOG:-}" ]; then
        printf '%s\n' "$line" >>"$CODEX_STUB_APP_SERVER_LOG"
      fi
      case "$line" in
        *'"method":"initialize"'*)
          printf '%s\n' '{"id":0,"result":{}}'
          ;;
        *'"method":"thread/start"'*)
          printf '%s\n' '{"id":1,"result":{"thread":{"id":"thr-owned-by-this-request"}}}'
          ;;
        *'"method":"thread/name/set"'*)
          if [ -n "${CODEX_STUB_NAME_ERROR:-}" ]; then
            printf '{"id":2,"error":{"code":-32000,"message":"%s"}}\n' \
              "$CODEX_STUB_NAME_ERROR"
          else
            printf '%s\n' '{"id":2,"result":{}}'
          fi
          ;;
        *'"method":"thread/delete"'*)
          if [ -n "${CODEX_STUB_DELETE_ERROR:-}" ]; then
            printf '{"id":3,"error":{"code":-32000,"message":"%s"}}\n' \
              "$CODEX_STUB_DELETE_ERROR"
          else
            printf '%s\n' '{"id":3,"result":{}}'
          fi
          ;;
      esac
    done
    exit 0
    ;;
  resume)
    log_path=${CODEX_STUB_RESUME_LOG:?CODEX_STUB_RESUME_LOG is required}
    {
      printf 'cwd=%s\000' "$PWD"
      for argument in "$@"; do
        printf 'arg=%s\000' "$argument"
      done
    } >"$log_path"
    exit "${CODEX_STUB_RESUME_EXIT_CODE:-0}"
    ;;
esac

printf 'unexpected codex fixture invocation: %s\n' "$*" >&2
exit 97
