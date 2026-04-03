# Language-neutral A/B conformance corpus for the pwf binary. One example per
# fixture dir under fixtures/: stage input, run the release binary with canonical
# args, and diff stdout (JSON subset) / stderr (substring) / output tree against
# the committed goldens. The per-fixture logic lives in spec_helper.sh::run_fixture.

Describe 'pwf conformance corpus'
  Parameters:dynamic
    for _d in "${SHELLSPEC_PROJECT_ROOT}"/fixtures/*/; do
      if [ -f "${_d}cmd.json" ]; then
        %data "${_d%/}"
      fi
    done
  End

  Example "fixture: $1"
    When call run_fixture "$1"
    The status should be success
    The output should equal ""
  End
End
