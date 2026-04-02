use gabagool::{Component, ComponentValue, Store};

#[test]
fn component_add() {
    let wasm = wat::parse_str(
        r#"
        (component
            (core module $m
                (func (export "add") (param i32 i32) (result i32)
                    local.get 0 local.get 1 i32.add)
            )
            (core instance $i (instantiate $m))
            (func (export "add") (param "a" s32) (param "b" s32) (result s32)
                (canon lift (core func $i "add"))
            )
        )
    "#,
    )
    .unwrap();

    let component = Component::new(&wasm).unwrap();

    let mut store = Store::new();
    let instance = store.instantiate_component(&component).unwrap();

    let results = store
        .invoke_component(
            instance,
            "add",
            vec![ComponentValue::S32(1), ComponentValue::S32(2)],
        )
        .unwrap()
        .into_completed()
        .unwrap();

    assert_eq!(results, vec![ComponentValue::S32(3)]);
}
