pub(crate) mod check_architecture;
pub(crate) mod install;
pub(crate) mod prepare;
pub(crate) mod ship;
pub(crate) mod test;

#[derive(Clone, Copy)]
pub(crate) struct Verb(&'static str);

impl Verb {
    pub(crate) const CHECK_ARCHITECTURE: Self = Self("check-architecture");
    pub(crate) const INSTALL: Self = Self("install");
    pub(crate) const PREPARE: Self = Self("prepare");
    pub(crate) const SHIP: Self = Self("ship");
    pub(crate) const TEST: Self = Self("test");
    pub(crate) const UPDATE: Self = Self("update");

    pub(crate) const fn as_str(self) -> &'static str {
        self.0
    }
}
