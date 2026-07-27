//! Repository-owned test declarations.

use std::{ffi::OsString, time::Duration};

use xtk_test::{Test, summary};

pub(crate) const E2E_TIMEOUT: Duration = Duration::from_hours(1);

pub(crate) struct TestDeclaration {
    #[cfg(test)]
    label: &'static str,
    test: Test,
}

impl TestDeclaration {
    fn new(label: &'static str, test: Test) -> Self {
        #[cfg(not(test))]
        let _ = label;
        Self {
            #[cfg(test)]
            label,
            test,
        }
    }

    #[cfg(test)]
    pub(crate) fn label(&self) -> &'static str {
        self.label
    }

    pub(crate) fn into_test(self) -> Test {
        self.test
    }
}

pub(crate) fn tests_unit() -> Vec<TestDeclaration> {
    vec![TestDeclaration::new(
        "unit",
        Test::new("unit", "cargo")
            .args(["test", "--quiet", "--workspace"])
            .verbose_arguments(["--", "--nocapture"])
            .summary_parser(summary::cargo),
    )]
}

pub(crate) fn tests_e2e(executable: OsString) -> Vec<TestDeclaration> {
    vec![TestDeclaration::new(
        "e2e",
        Test::new("e2e", executable)
            .arg("e2e-worker")
            .verbose_arguments(["--verbose"])
            .accepts_evidences()
            .timeout(E2E_TIMEOUT),
    )]
}

pub(crate) fn tests_all(executable: OsString) -> Vec<TestDeclaration> {
    let mut tests = tests_unit();
    tests.extend(tests_e2e(executable.clone()));
    tests.extend([
        TestDeclaration::new(
            "architecture",
            Test::new("architecture", executable).arg("check-architecture"),
        ),
        TestDeclaration::new(
            "ast-rules",
            Test::new("ast-rules", "ast-grep").args(["test", "--skip-snapshot-tests"]),
        ),
        TestDeclaration::new(
            "ast-scan",
            Test::new("ast-scan", "ast-grep").args(["scan", "--globs", "!xtask/xtk_test/**"]),
        ),
    ]);
    tests
}
