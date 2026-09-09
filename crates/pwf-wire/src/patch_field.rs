/// Selects how one resource field changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum PatchField<T> {
    /// Leaves the stored field unchanged.
    #[default]
    NoAction,
    /// Replaces the stored field.
    Set(T),
    /// Removes the stored field.
    Clear,
}

impl<T> PatchField<T> {
    /// Applies this update and returns the replaced or cleared value.
    /// The caller owns its destruction, including when evaluating this method in a const context.
    #[inline]
    pub const fn apply(self, target: &mut Option<T>) -> Option<T> {
        let previous = match &self {
            Self::NoAction => None,
            Self::Set(value) => {
                // SAFETY: replace cannot panic, and forgetting self below prevents a second drop of
                // the moved value.
                target.replace(unsafe { std::ptr::read(value) })
            }
            Self::Clear => target.take(),
        };
        std::mem::forget(self);
        previous
    }

    pub(crate) fn is_unchanged(&self) -> bool {
        matches!(self, Self::NoAction)
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use super::PatchField;

    #[test]
    fn updates_are_consumed_in_constant_expressions() {
        const RESULT: (Option<u32>, Option<u32>, Option<u32>, Option<u32>) = {
            let mut value = Some(1);
            let unchanged = PatchField::NoAction.apply(&mut value);
            let replaced = PatchField::Set(2).apply(&mut value);
            let cleared = PatchField::Clear.apply(&mut value);
            (value, unchanged, replaced, cleared)
        };
        const OWNED: (Option<String>, Option<String>) = {
            let mut value = None;
            let previous = PatchField::Set(String::new()).apply(&mut value);
            (value, previous)
        };
        assert_eq!(RESULT, (None, None, Some(1), Some(2)));
        assert_eq!(OWNED, (Some(String::new()), None));
    }

    struct Tracked {
        text: String,
        drops: Rc<Cell<u32>>,
    }

    impl Drop for Tracked {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
        }
    }

    #[test]
    fn moving_updates_preserves_allocations_and_drops_each_value_once() {
        let drops = Rc::new(Cell::new(0));
        let mut value = Some(Tracked {
            text: "old".into(),
            drops: Rc::clone(&drops),
        });
        assert!(PatchField::NoAction.apply(&mut value).is_none());
        assert_eq!(value.as_ref().unwrap().text, "old");
        let replacement = Tracked {
            text: "new".into(),
            drops: Rc::clone(&drops),
        };
        let allocation = replacement.text.as_ptr();
        let previous = PatchField::Set(replacement).apply(&mut value);
        assert_eq!(value.as_ref().unwrap().text.as_ptr(), allocation);
        assert_eq!(previous.as_ref().unwrap().text, "old");
        assert_eq!(drops.get(), 0);
        drop(previous);
        assert_eq!(drops.get(), 1);
        drop(PatchField::Clear.apply(&mut value));
        assert!(value.is_none());
        assert_eq!(drops.get(), 2);
        assert!(PatchField::Clear.apply(&mut value).is_none());
        assert!(PatchField::NoAction.apply(&mut value).is_none());
        let replacement = Tracked {
            text: "last".into(),
            drops: Rc::clone(&drops),
        };
        assert!(PatchField::Set(replacement).apply(&mut value).is_none());
        drop(value);
        assert_eq!(drops.get(), 3);
    }
}
