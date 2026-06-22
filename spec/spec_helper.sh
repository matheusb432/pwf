# ShellSpec helpers for pwf recipe-behavior specs.

# Run `just pwf test <flags>` with a fake `cargo` shimmed onto PATH that records
# its invocation instead of compiling/running the suite. Sets CARGO_INVOCATION to
# the recorded `cargo` arg line (empty if cargo was never reached) and returns
# just's own exit status — so specs assert the flag contract (terse-by-default)
# without a real, slow test run.
run_just_test() {
  cd "$SHELLSPEC_PROJECT_ROOT" || return 1
  shimdir=$(mktemp -d)
  export PWF_SPEC_CARGOLOG="$shimdir/cargo.log"
  : >"$PWF_SPEC_CARGOLOG"
  cat >"$shimdir/cargo" <<'SHIM'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$PWF_SPEC_CARGOLOG"
SHIM
  chmod +x "$shimdir/cargo"
  PATH="$shimdir:$PATH" just pwf test "$@"
  status=$?
  CARGO_INVOCATION=$(cat "$PWF_SPEC_CARGOLOG")
  rm -rf "$shimdir"
  return $status
}
