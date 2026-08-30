use pwf_client::pb::{
    ClearField, StringCollectionEdit, StringFieldUpdate, StringValues, string_collection_edit,
    string_field_update,
};

pub(crate) fn string_collection_edit(
    values: Vec<String>,
    remove_existing: bool,
) -> Option<StringCollectionEdit> {
    match (values.is_empty(), remove_existing) {
        (false, true) => Some(StringCollectionEdit {
            operation: Some(string_collection_edit::Operation::Replace(StringValues {
                values,
            })),
        }),
        (false, false) => Some(StringCollectionEdit {
            operation: Some(string_collection_edit::Operation::Append(StringValues {
                values,
            })),
        }),
        (true, true) => Some(StringCollectionEdit {
            operation: Some(string_collection_edit::Operation::Clear(ClearField {})),
        }),
        (true, false) => None,
    }
}

pub(crate) fn string_field_edit(
    value: Option<String>,
    remove_existing: bool,
) -> Option<StringFieldUpdate> {
    value.map_or_else(
        || {
            remove_existing.then_some(StringFieldUpdate {
                operation: Some(string_field_update::Operation::Clear(ClearField {})),
            })
        },
        |value| {
            Some(StringFieldUpdate {
                operation: Some(string_field_update::Operation::Update(value)),
            })
        },
    )
}

#[cfg(test)]
mod tests {
    use pwf_client::pb;

    use super::{string_collection_edit, string_field_edit};

    #[test]
    fn collection_additions_append_unless_removal_requests_replacement() {
        let appended = string_collection_edit(vec!["new".to_string()], false).unwrap();
        assert!(matches!(
            appended.operation,
            Some(pb::string_collection_edit::Operation::Append(_))
        ));

        let replaced = string_collection_edit(vec!["new".to_string()], true).unwrap();
        assert!(matches!(
            replaced.operation,
            Some(pb::string_collection_edit::Operation::Replace(_))
        ));

        let cleared = string_collection_edit(Vec::new(), true).unwrap();
        assert!(matches!(
            cleared.operation,
            Some(pb::string_collection_edit::Operation::Clear(_))
        ));
        assert!(string_collection_edit(Vec::new(), false).is_none());
    }

    #[test]
    fn scalar_removal_requires_an_explicit_clear_flag() {
        assert!(matches!(
            string_field_edit(Some("value".to_string()), false)
                .unwrap()
                .operation,
            Some(pb::string_field_update::Operation::Update(_))
        ));
        assert!(matches!(
            string_field_edit(None, true).unwrap().operation,
            Some(pb::string_field_update::Operation::Clear(_))
        ));
        assert!(string_field_edit(None, false).is_none());
    }
}
