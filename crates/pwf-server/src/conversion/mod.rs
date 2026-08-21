pub(crate) mod input;
pub(crate) mod output;

use std::{fmt::Display, str::FromStr};

use tonic::Status;

pub(crate) fn invalid(field: &str, reason: impl Display) -> Status {
    Status::invalid_argument(format!("invalid {field}: {reason}"))
}

pub(crate) fn parse<T>(field: &str, value: &str) -> Result<T, Status>
where
    T: FromStr,
    T::Err: Display,
{
    value.parse().map_err(|error| invalid(field, error))
}

pub(crate) fn required<T>(field: &str, value: Option<T>) -> Result<T, Status> {
    value.ok_or_else(|| Status::invalid_argument(format!("{field} is required")))
}
