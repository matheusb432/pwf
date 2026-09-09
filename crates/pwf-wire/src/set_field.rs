/// Selects an update to a field that cannot be cleared.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SetField<T> {
    #[default]
    NoAction,
    Set(T),
}

impl<T> SetField<T> {
    #[must_use]
    pub const fn is_unchanged(&self) -> bool {
        matches!(self, Self::NoAction)
    }

    #[must_use]
    pub const fn as_ref(&self) -> SetField<&T> {
        match self {
            Self::NoAction => SetField::NoAction,
            Self::Set(value) => SetField::Set(value),
        }
    }

    #[must_use]
    pub fn map<U>(self, map: impl FnOnce(T) -> U) -> SetField<U> {
        match self {
            Self::NoAction => SetField::NoAction,
            Self::Set(value) => SetField::Set(map(value)),
        }
    }

    pub fn apply(self, target: &mut T) {
        if let Self::Set(value) = self {
            *target = value;
        }
    }
}

/// Interprets absence as no action and presence as replacement.
impl<T> From<Option<T>> for SetField<T> {
    fn from(value: Option<T>) -> Self {
        match value {
            None => Self::NoAction,
            Some(value) => Self::Set(value),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SetField;

    #[test]
    fn omission_preserves_the_value_and_replacement_moves_its_allocation() {
        let mut title = String::from("original title");
        let original_allocation = title.as_ptr();
        SetField::from(None).apply(&mut title);
        assert_eq!(title, "original title");
        assert_eq!(title.as_ptr(), original_allocation);

        let replacement = String::from("updated title");
        let replacement_allocation = replacement.as_ptr();
        SetField::from(Some(replacement)).apply(&mut title);
        assert_eq!(title, "updated title");
        assert_eq!(title.as_ptr(), replacement_allocation);
    }
}
