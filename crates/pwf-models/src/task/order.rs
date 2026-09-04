use std::{fmt, str::FromStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderField {
    Created,
    Id,
    ProjectId,
    Priority,
    Effort,
    Title,
}

impl AsRef<str> for OrderField {
    fn as_ref(&self) -> &str {
        match self {
            Self::Created => "created",
            Self::Id => "id",
            Self::ProjectId => "project-id",
            Self::Priority => "priority",
            Self::Effort => "effort",
            Self::Title => "title",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderDirection {
    Asc,
    Desc,
}

impl AsRef<str> for OrderDirection {
    fn as_ref(&self) -> &str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderSpec {
    pub field: OrderField,
    pub direction: OrderDirection,
}

impl Default for OrderSpec {
    fn default() -> Self {
        Self {
            field: OrderField::Id,
            direction: OrderDirection::Desc,
        }
    }
}

impl FromStr for OrderSpec {
    type Err = OrderSpecError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (field, direction) = value
            .split_once(':')
            .map_or((value, None), |(field, direction)| (field, Some(direction)));
        let field = match field {
            "created" => OrderField::Created,
            "id" => OrderField::Id,
            "project-id" => OrderField::ProjectId,
            "priority" => OrderField::Priority,
            "effort" => OrderField::Effort,
            "title" => OrderField::Title,
            _ => return Err(OrderSpecError),
        };
        let direction = match direction {
            Some("asc") => OrderDirection::Asc,
            Some("desc") => OrderDirection::Desc,
            None => match field {
                OrderField::Created | OrderField::Id | OrderField::Priority => OrderDirection::Desc,
                OrderField::ProjectId | OrderField::Effort | OrderField::Title => {
                    OrderDirection::Asc
                }
            },
            Some(_) => return Err(OrderSpecError),
        };
        Ok(Self { field, direction })
    }
}

impl fmt::Display for OrderSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}:{}",
            self.field.as_ref(),
            self.direction.as_ref()
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error(
    "use field[:direction] with field created|id|project-id|priority|effort|title and direction asc|desc"
)]
pub struct OrderSpecError;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_names_have_field_specific_defaults_and_explicit_overrides() {
        for (field, default) in [
            ("created", OrderDirection::Desc),
            ("id", OrderDirection::Desc),
            ("project-id", OrderDirection::Asc),
            ("priority", OrderDirection::Desc),
            ("effort", OrderDirection::Asc),
            ("title", OrderDirection::Asc),
        ] {
            let order: OrderSpec = field.parse().unwrap();
            assert_eq!(order.direction, default);
            assert_eq!(order.field.as_ref(), field);
            for direction in [OrderDirection::Asc, OrderDirection::Desc] {
                let explicit = OrderSpec {
                    field: order.field,
                    direction,
                };
                assert_eq!(explicit.to_string().parse(), Ok(explicit));
            }
        }
        assert_eq!("id".parse(), Ok(OrderSpec::default()));
    }

    #[test]
    fn malformed_sort_keys_are_rejected() {
        for value in [
            "",
            "asc",
            "bogus",
            "id:",
            "priority:sideways",
            "title:asc:desc",
            " id",
            "ID",
        ] {
            assert!(value.parse::<OrderSpec>().is_err(), "{value}");
        }
    }
}
