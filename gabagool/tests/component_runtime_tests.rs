use gabagool::{Component, ComponentValue, ExecutionState, Store};

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

#[test]
fn component_string_length() {
    let wasm = wat::parse_str(
        r#"
        (component
            (core module $m
                (memory (export "memory") 1)

                (global $bump (mut i32) (i32.const 0))
                (func (export "realloc") (param i32 i32 i32 i32) (result i32)
                    (local $ptr i32)
                    global.get $bump
                    local.set $ptr
                    global.get $bump
                    local.get 3
                    i32.add
                    global.set $bump
                    local.get $ptr
                )

                (func (export "string-length") (param i32 i32) (result i32)
                    local.get 1
                )
            )
            (core instance $i (instantiate $m))
            (func (export "string-length") (param "s" string) (result u32)
                (canon lift (core func $i "string-length")
                    (memory $i "memory")
                    (realloc (func $i "realloc"))
                )
            )
        )
    "#,
    )
    .unwrap();

    let component = Component::new(&wasm).unwrap();
    let mut store = Store::new();
    let instance = store.instantiate_component(&component).unwrap();

    let results = store
        .invoke_component(instance, "string-length", vec!["howdy"])
        .unwrap()
        .into_completed()
        .unwrap();

    assert_eq!(results, vec![ComponentValue::U32(5)]);
}

#[test]
fn component_string_identity() {
    let wasm = wat::parse_str(
        r#"
        (component
            (core module $m
                (memory (export "memory") 1)

                (global $bump (mut i32) (i32.const 0))
                (func (export "realloc") (param i32 i32 i32 i32) (result i32)
                    (local $ptr i32)
                    global.get $bump
                    local.set $ptr
                    global.get $bump
                    local.get 3
                    i32.add
                    global.set $bump
                    local.get $ptr
                )

                (func (export "identity") (param $ptr i32) (param $len i32) (param $retptr i32)
                    local.get $retptr
                    local.get $ptr
                    i32.store

                    local.get $retptr
                    local.get $len
                    i32.store offset=4
                )
            )
            (core instance $i (instantiate $m))
            (func (export "identity") (param "s" string) (result string)
                (canon lift (core func $i "identity")
                    (memory $i "memory")
                    (realloc (func $i "realloc"))
                )
            )
        )
    "#,
    )
    .unwrap();

    let component = Component::new(&wasm).unwrap();
    let mut store = Store::new();
    let instance = store.instantiate_component(&component).unwrap();

    let results = store
        .invoke_component(instance, "identity", vec!["howdy world"])
        .unwrap()
        .into_completed()
        .unwrap();

    assert_eq!(
        results,
        vec![ComponentValue::String("howdy world".to_string())]
    );
}

#[test]
fn component_snapshot_add() {
    let wasm = wat::parse_str(
        r#"
        (component
            (core module $m
                (func (export "sum") (param i32) (result i32)
                    (local $i i32)
                    (local $acc i32)
                    (block $break
                        (loop $loop
                            local.get $i
                            local.get 0
                            i32.ge_s
                            br_if $break

                            local.get $acc
                            local.get $i
                            i32.add
                            local.set $acc

                            local.get $i
                            i32.const 1
                            i32.add
                            local.set $i

                            br $loop
                        )
                    )
                    local.get $acc
                )
            )
            (core instance $i (instantiate $m))
            (func (export "sum") (param "n" s32) (result s32)
                (canon lift (core func $i "sum"))
            )
        )
    "#,
    )
    .unwrap();

    let component = Component::new(&wasm).unwrap();
    let mut store = Store::new();
    let instance = store.instantiate_component(&component).unwrap();

    store.set_fuel(10);

    let state = store.invoke_component(instance, "sum", vec![100]).unwrap();

    assert!(matches!(state, ExecutionState::FuelExhausted));

    let snapshot = store.snapshot();
    let mut restored = Store::from_snapshot(&snapshot);
    restored.set_fuel(u64::MAX);

    let result = restored
        .resume_component()
        .unwrap()
        .into_completed()
        .unwrap();

    assert_eq!(result, vec![ComponentValue::S32((0..100).sum())]);
}

#[test]
fn component_list_sum() {
    let wasm = wat::parse_str(
        r#"
        (component
            (core module $m
                (memory (export "memory") 1)

                (global $bump (mut i32) (i32.const 0))
                (func (export "realloc") (param i32 i32 i32 i32) (result i32)
                    (local $ptr i32)
                    global.get $bump
                    local.set $ptr
                    global.get $bump
                    local.get 3
                    i32.add
                    global.set $bump
                    local.get $ptr
                )

                (func (export "sum") (param $ptr i32) (param $len i32) (result i32)
                    (local $i i32)
                    (local $acc i32)
                    (block $break
                        (loop $loop
                            local.get $i
                            local.get $len
                            i32.ge_s
                            br_if $break

                            local.get $acc
                            local.get $ptr
                            local.get $i
                            i32.const 4
                            i32.mul
                            i32.add
                            i32.load
                            i32.add
                            local.set $acc

                            local.get $i
                            i32.const 1
                            i32.add
                            local.set $i

                            br $loop
                        )
                    )
                    local.get $acc
                )
            )
            (core instance $i (instantiate $m))
            (func (export "sum") (param "values" (list s32)) (result s32)
                (canon lift (core func $i "sum")
                    (memory $i "memory")
                    (realloc (func $i "realloc"))
                )
            )
        )
    "#,
    )
    .unwrap();

    let component = Component::new(&wasm).unwrap();
    let mut store = Store::new();
    let instance = store.instantiate_component(&component).unwrap();

    let list = [10, 20, 30];

    let results = store
        .invoke_component(instance, "sum", vec![list])
        .unwrap()
        .into_completed()
        .unwrap();

    assert_eq!(results, vec![ComponentValue::S32(60)]);
}

#[test]
fn component_list_identity() {
    let wasm = wat::parse_str(
        r#"
        (component
            (core module $m
                (memory (export "memory") 1)

                (global $bump (mut i32) (i32.const 0))
                (func (export "realloc") (param i32 i32 i32 i32) (result i32)
                    (local $ptr i32)
                    global.get $bump
                    local.set $ptr
                    global.get $bump
                    local.get 3
                    i32.add
                    global.set $bump
                    local.get $ptr
                )

                (func (export "identity") (param $ptr i32) (param $len i32) (param $retptr i32)
                    local.get $retptr
                    local.get $ptr
                    i32.store

                    local.get $retptr
                    local.get $len
                    i32.store offset=4
                )
            )
            (core instance $i (instantiate $m))
            (func (export "identity") (param "values" (list u8)) (result (list u8))
                (canon lift (core func $i "identity")
                    (memory $i "memory")
                    (realloc (func $i "realloc"))
                )
            )
        )
    "#,
    )
    .unwrap();

    let component = Component::new(&wasm).unwrap();
    let mut store = Store::new();
    let instance = store.instantiate_component(&component).unwrap();

    let results = store
        .invoke_component(instance, "identity", vec![&[1u8, 2, 3]])
        .unwrap()
        .into_completed()
        .unwrap();

    assert_eq!(
        results,
        vec![ComponentValue::List(vec![
            ComponentValue::U8(1),
            ComponentValue::U8(2),
            ComponentValue::U8(3),
        ])]
    );
}

#[test]
fn component_record_add_point() {
    let wasm = wat::parse_str(
        r#"
        (component
            (core module $m
                (func (export "add") (param i32 i32 i32 i32) (result i32)
                    local.get 0 local.get 2 i32.add
                    local.get 1 local.get 3 i32.add
                    i32.add
                )
            )
            (core instance $i (instantiate $m))
            (func (export "add") (param "a" (record (field "x" s32) (field "y" s32))) (param "b" (record (field "x" s32) (field "y" s32))) (result s32)
                (canon lift (core func $i "add"))
            )
        )
    "#,
    )
    .unwrap();

    let component = Component::new(&wasm).unwrap();
    let mut store = Store::new();
    let instance = store.instantiate_component(&component).unwrap();

    let a = vec![("x", 1), ("y", 2)];
    let b = vec![("x", 3), ("y", 4)];

    let results = store
        .invoke_component(instance, "add", vec![a, b])
        .unwrap()
        .into_completed()
        .unwrap();

    assert_eq!(results, vec![ComponentValue::S32(10)]);
}

#[test]
fn component_tuple_swap() {
    let wasm = wat::parse_str(
        r#"
        (component
            (core module $m
                (memory (export "memory") 1)

                (global $bump (mut i32) (i32.const 0))
                (func (export "realloc") (param i32 i32 i32 i32) (result i32)
                    (local $ptr i32)
                    global.get $bump
                    local.set $ptr
                    global.get $bump
                    local.get 3
                    i32.add
                    global.set $bump
                    local.get $ptr
                )

                (func (export "swap") (param i32 i32) (param i32)
                    ;; write second to retptr[0], first to retptr[4]
                    local.get 2
                    local.get 1
                    i32.store

                    local.get 2
                    local.get 0
                    i32.store offset=4
                )
            )
            (core instance $i (instantiate $m))
            (func (export "swap") (param "p" (tuple s32 s32)) (result (tuple s32 s32))
                (canon lift (core func $i "swap")
                    (memory $i "memory")
                    (realloc (func $i "realloc"))
                )
            )
        )
    "#,
    )
    .unwrap();

    let component = Component::new(&wasm).unwrap();
    let mut store = Store::new();
    let instance = store.instantiate_component(&component).unwrap();

    // a convenience macro for tuples?
    let pair = ComponentValue::Tuple(vec![ComponentValue::S32(10), ComponentValue::S32(20)]);

    let results = store
        .invoke_component(instance, "swap", vec![pair])
        .unwrap()
        .into_completed()
        .unwrap();

    assert_eq!(
        results,
        vec![ComponentValue::Tuple(vec![
            ComponentValue::S32(20),
            ComponentValue::S32(10),
        ])]
    );
}

#[test]
fn component_option_double() {
    let wasm = wat::parse_str(
        r#"
        (component
            (core module $m
                (memory (export "memory") 1)
                (global $bump (mut i32) (i32.const 0))
                (func (export "realloc") (param i32 i32 i32 i32) (result i32)
                    (local $ptr i32)
                    global.get $bump
                    local.set $ptr
                    global.get $bump
                    local.get 3
                    i32.add
                    global.set $bump
                    local.get $ptr
                )

                (func (export "double") (param i32 i32) (param i32)
                    local.get 2
                    local.get 0
                    i32.store

                    local.get 2
                    local.get 1
                    i32.const 2
                    i32.mul
                    local.get 0
                    i32.mul
                    i32.store offset=4
                )
            )
            (core instance $i (instantiate $m))
            (func (export "double") (param "v" (option s32)) (result (option s32))
                (canon lift (core func $i "double")
                    (memory $i "memory")
                    (realloc (func $i "realloc"))
                )
            )
        )
    "#,
    )
    .unwrap();

    let component = Component::new(&wasm).unwrap();
    let mut store = Store::new();
    let instance = store.instantiate_component(&component).unwrap();

    let results = store
        .invoke_component(instance, "double", vec![Some(21)])
        .unwrap()
        .into_completed()
        .unwrap();

    assert_eq!(
        results,
        vec![ComponentValue::Option(Some(Box::new(ComponentValue::S32(
            42
        ))))]
    );

    let results = store
        .invoke_component(instance, "double", vec![None::<i32>])
        .unwrap()
        .into_completed()
        .unwrap();

    assert_eq!(results, vec![ComponentValue::Option(None)]);
}

#[test]
fn component_result_safe_div() {
    let wasm = wat::parse_str(
        r#"
        (component
            (core module $m
                (memory (export "memory") 1)
                (global $bump (mut i32) (i32.const 0))
                (func (export "realloc") (param i32 i32 i32 i32) (result i32)
                    (local $ptr i32)
                    global.get $bump
                    local.set $ptr
                    global.get $bump
                    local.get 3
                    i32.add
                    global.set $bump
                    local.get $ptr
                )

                (func (export "safe-div") (param i32 i32) (param i32)
                    (if (i32.eqz (local.get 1))
                        (then
                            local.get 2
                            i32.const 1
                            i32.store

                            local.get 2
                            i32.const 0
                            i32.store offset=4
                        )
                        (else
                            local.get 2
                            i32.const 0
                            i32.store

                            local.get 2
                            local.get 0
                            local.get 1
                            i32.div_s
                            i32.store offset=4
                        )
                    )
                )
            )
            (core instance $i (instantiate $m))
            (func (export "safe-div") (param "a" s32) (param "b" s32) (result (result s32 (error s32)))
                (canon lift (core func $i "safe-div")
                    (memory $i "memory")
                    (realloc (func $i "realloc"))
                )
            )
        )
    "#,
    )
    .unwrap();

    let component = Component::new(&wasm).unwrap();
    let mut store = Store::new();
    let instance = store.instantiate_component(&component).unwrap();

    // 10 / 2 = Ok(5)
    let results = store
        .invoke_component(instance, "safe-div", vec![10, 2])
        .unwrap()
        .into_completed()
        .unwrap();

    assert_eq!(
        results,
        vec![ComponentValue::Result(Ok(Some(Box::new(
            ComponentValue::S32(5)
        ))))]
    );

    let results = store
        .invoke_component(instance, "safe-div", vec![10, 0])
        .unwrap()
        .into_completed()
        .unwrap();

    assert_eq!(
        results,
        vec![ComponentValue::Result(Err(Some(Box::new(
            ComponentValue::S32(0)
        ))))]
    );
}
