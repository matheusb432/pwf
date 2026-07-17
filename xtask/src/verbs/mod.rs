//! One module per automation verb.
//!
//! A new verb needs a module, a [`crate::cli::Command`] arm, and dispatch in `main.rs`.

pub(crate) mod check_architecture;
pub(crate) mod fmt;
pub(crate) mod install;
pub(crate) mod test;
