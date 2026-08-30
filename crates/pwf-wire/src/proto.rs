//! Explicit mappings between process-neutral contracts and protobuf messages.

use std::{fmt::Display, str::FromStr};

use tonic::Status;

use crate::{collection_edit::CollectionEdit, v1};

pub mod note;
pub mod project;
pub mod session;
pub mod task;

fn invalid(field: &str, reason: impl Display) -> Status {
    Status::invalid_argument(format!("invalid {field}: {reason}"))
}

fn parse<T>(field: &str, value: &str) -> Result<T, Status>
where
    T: FromStr,
    T::Err: Display,
{
    value.parse().map_err(|error| invalid(field, error))
}

fn required<T>(field: &str, value: Option<T>) -> Result<T, Status> {
    value.ok_or_else(|| Status::invalid_argument(format!("{field} is required")))
}

fn collection_edit<T>(
    value: Option<v1::StringCollectionEdit>,
    parse_values: fn(Vec<String>) -> Result<Option<T>, Status>,
) -> Result<CollectionEdit<T>, Status> {
    let Some(value) = value else {
        return Ok(CollectionEdit::Unchanged);
    };
    match required("collection_edit.operation", value.operation)? {
        v1::string_collection_edit::Operation::Append(values) => parse_values(values.values)?
            .map(CollectionEdit::Append)
            .ok_or_else(|| invalid("collection_edit.values", "cannot be empty")),
        v1::string_collection_edit::Operation::Replace(values) => parse_values(values.values)?
            .map(CollectionEdit::Replace)
            .ok_or_else(|| invalid("collection_edit.values", "cannot be empty")),
        v1::string_collection_edit::Operation::Clear(_) => Ok(CollectionEdit::Clear),
    }
}
