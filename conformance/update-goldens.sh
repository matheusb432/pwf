#!/bin/sh
# Regenerate the conformance goldens (expected/) for every fixture from the current
# release binary. Run after an intentional behavior change, then review the diff.
# Verification is `shellspec` (or `just test-conformance`); this only rewrites goldens.
set -u

here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
export SHELLSPEC_PROJECT_ROOT="$here"
# shellcheck source=conformance/spec/spec_helper.sh
. "$here/spec/spec_helper.sh"

[ -x "$(cf_bin)" ] || { echo "release binary not found at $(cf_bin); run 'just build'." >&2; exit 1; }

for d in "$here"/fixtures/*/; do
  [ -f "${d}cmd.json" ] || continue
  update_fixture "${d%/}"
done
