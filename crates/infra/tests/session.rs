use pwf_infra::session::{ProcessSessionRuntime, TomlModelTierCatalog};

#[test]
fn session_adapters_are_publicly_constructible() {
    fn assert_adapter<T: Clone + Send + Sync + 'static>() {}

    assert_adapter::<ProcessSessionRuntime>();
    assert_adapter::<TomlModelTierCatalog>();
}
