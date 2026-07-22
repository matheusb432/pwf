//! Verb names.

#[derive(Clone, Copy)]
pub(crate) struct Verb(&'static str);

impl Verb {
    pub(crate) const CHECK: Self = Self("check");
    pub(crate) const CHECK_ARCHITECTURE: Self = Self("check-architecture");
    pub(crate) const FIX: Self = Self("fix");
    pub(crate) const FORMAT: Self = Self("fmt");
    pub(crate) const FORMAT_CHECK: Self = Self("fmt-check");
    pub(crate) const INSTALL: Self = Self("install");
    pub(crate) const LINT: Self = Self("lint");
    pub(crate) const SHIP: Self = Self("ship");
    pub(crate) const TEST: Self = Self("test");
    pub(crate) const UPDATE: Self = Self("update");

    pub(crate) const fn as_str(self) -> &'static str {
        self.0
    }
}

impl std::fmt::Display for Verb {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}
