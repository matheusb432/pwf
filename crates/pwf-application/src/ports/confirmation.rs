use futures::future::BoxFuture;

use crate::contract::confirmation::Confirmation;

pub trait ConfirmationClient {
    fn confirm<'a>(&'a self, confirmation: &'a Confirmation) -> BoxFuture<'a, bool>;
}
