//! One module per automation verb.
//!
//! To add a verb: create a module here with a `run` function (plus a clap `Args` struct when it
//! takes flags), then add a [`crate::cli::Command`] arm and its dispatch line in `main.rs`.

pub(crate) mod fmt;
pub(crate) mod install;
pub(crate) mod smell_check;
pub(crate) mod test;
