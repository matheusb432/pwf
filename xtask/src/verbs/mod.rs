//! One module per automation verb.
//!
//! A new verb needs a module, a [`crate::cli::Command`] arm, and dispatch in `main.rs`.

pub(crate) mod check;
pub(crate) mod check_architecture;
pub(crate) mod format;
pub(crate) mod install;
pub(crate) mod lint;
pub(crate) mod ship;
pub(crate) mod test;
