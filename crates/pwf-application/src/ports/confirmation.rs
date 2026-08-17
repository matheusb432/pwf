use pwf_wire::confirmation::Confirmation;

pub trait ConfirmationClient {
    fn confirm(&self, confirmation: &Confirmation) -> bool;
}
