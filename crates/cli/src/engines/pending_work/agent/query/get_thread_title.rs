use crate::engines::pending_work::Item;

#[derive(Clone, Copy)]
pub struct GetThreadTitle<'a> {
    work_item_id: &'a str,
    task_title: &'a str,
}

impl<'a> From<&'a Item> for GetThreadTitle<'a> {
    fn from(value: &'a Item) -> Self {
        Self {
            work_item_id: &value.id,
            task_title: &value.session,
        }
    }
}

pub fn handle(request: GetThreadTitle) -> String {
    format!("{} - {}", request.work_item_id, request.task_title)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_maps_title() {
        let mut item = Item::empty();
        item.id = "PWF-0001".to_string();
        item.session = "create a new feature".to_string();

        let request = (&item).into();

        let res = super::handle(request);

        assert_eq!(res, "PWF-0001 - create a new feature");
    }
}
