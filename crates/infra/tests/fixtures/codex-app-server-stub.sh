#!/bin/sh
# Implements one isolated Codex app-server connection for infrastructure tests.
set -eu

if [ "${1:-}" != "app-server" ] || [ "${2:-}" != "--stdio" ]; then
  printf 'unexpected Codex app-server fixture invocation: %s\n' "$*" >&2
  exit 97
fi

fixture_directory=${0%/*}
log_path=$fixture_directory/requests.jsonl

while IFS= read -r line; do
  printf '%s\n' "$line" >>"$log_path"
  case "$line" in
    *'"method":"initialize"'*)
      if [ -f "$fixture_directory/oversized-response-line" ]; then
        dd if=/dev/zero bs=1048577 count=1 2>/dev/null | tr '\000' x
        printf '\n'
      else
        printf '%s\n' '{"id":0,"result":{}}'
      fi
      ;;
    *'"method":"thread/start"'*)
      printf '%s\n' '{"method":"thread/started","params":{"thread":{"id":"thr-created-concurrently"}}}'
      printf '%s\n' '{"id":999,"result":{"thread":{"id":"thr-existing-before"}}}'
      printf '%s\n' '{"id":1,"result":{"thread":{"id":"thr-owned-by-this-request"}}}'
      ;;
    *'"method":"thread/name/set"'*)
      if [ -f "$fixture_directory/naming-fails" ]; then
        printf '%s\n' '{"id":2,"error":{"code":-32000,"message":"name denied by fixture"}}'
      else
        printf '%s\n' '{"id":2,"result":{}}'
      fi
      ;;
    *'"method":"thread/delete"'*)
      if [ -f "$fixture_directory/cleanup-fails" ]; then
        printf '%s\n' '{"id":3,"error":{"code":-32000,"message":"delete denied by fixture"}}'
      else
        printf '%s\n' '{"id":3,"result":{}}'
      fi
      ;;
  esac
done
