Describe 'just pwf test (flag contract)'
  # Guards the terse-by-default contract: the recipe must pass `cargo --quiet`
  # unless `--verbose` is given, in which case it drops `--quiet` and adds
  # `-- --nocapture` for full per-test output. cargo is shimmed (see spec_helper),
  # so these assert the invocation, not a real suite run.

  It 'defaults to terse output (cargo --quiet, no --nocapture)'
    When call run_just_test
    The status should be success
    The variable CARGO_INVOCATION should eq "test --quiet"
  End

  It 'expands to full output under --verbose (drops --quiet, adds --nocapture)'
    When call run_just_test --verbose
    The status should be success
    The variable CARGO_INVOCATION should eq "test -- --nocapture"
  End

  It 'rejects unknown flags before reaching cargo'
    When call run_just_test --bogus
    The status should be failure
    The variable CARGO_INVOCATION should eq ""
    The error should include "unknown flag"
  End
End
