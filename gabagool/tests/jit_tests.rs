#![cfg(feature = "jit")]

use gabagool::{Module, RawValue, Store};

fn jit_run(wat: &str, func: &str, args: Vec<RawValue>) -> Vec<RawValue> {
    let wasm = wat::parse_str(wat).unwrap();
    let module = Module::new(&wasm).unwrap();
    let mut store = Store::new();
    let instance = store.instantiate(&module, vec![]).unwrap();
    store
        .invoke(instance, func, args)
        .unwrap()
        .into_completed()
        .unwrap()
}

#[test]
fn i32_const() {
    let result = jit_run(
        r#"(module (func (export "f") (result i32) i32.const 42))"#,
        "f",
        vec![],
    );
    assert_eq!(result[0].as_i32(), 42);
}

#[test]
fn nop_then_const() {
    let result = jit_run(
        r#"(module (func (export "f") (result i32) nop nop i32.const 7))"#,
        "f",
        vec![],
    );
    assert_eq!(result[0].as_i32(), 7);
}

#[test]
fn local_set_and_get() {
    let result = jit_run(
        r#"(module (func (export "f") (result i32)
            (local i32)
            i32.const 67
            local.set 0
            local.get 0
        ))"#,
        "f",
        vec![],
    );
    assert_eq!(result[0].as_i32(), 67);
}
