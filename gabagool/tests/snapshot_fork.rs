#![cfg(all(unix, not(any(feature = "core-tests", feature = "component-tests"))))]

use gabagool::{Module, RawValue, Store};

const TINY_MEM_WAT: &str = r#"
(module
    (memory (export "mem") 1 16)
    (func (export "write_u32") (param $offset i32) (param $value i32)
        local.get $offset
        local.get $value
        i32.store)
    (func (export "read_u32") (param $offset i32) (result i32)
        local.get $offset
        i32.load))
"#;

fn build_tiny_mem_store() -> (Store, gabagool::Instance) {
    let wasm = wat::parse_str(TINY_MEM_WAT).unwrap();
    let module = Module::new(&wasm).unwrap();
    let mut store = Store::new_cow();
    let instance = store.instantiate(&module, vec![]).unwrap();
    (store, instance)
}

fn write(store: &mut Store, instance: gabagool::Instance, offset: i32, value: i32) {
    store
        .invoke(
            instance,
            "write_u32",
            vec![RawValue::from(offset), RawValue::from(value)],
        )
        .unwrap()
        .into_completed()
        .unwrap();
}

fn read(store: &mut Store, instance: gabagool::Instance, offset: i32) -> i32 {
    store
        .invoke(instance, "read_u32", vec![RawValue::from(offset)])
        .unwrap()
        .into_completed()
        .unwrap()[0]
        .as_i32()
}

#[test]
fn fork_children_inherit_parent_memory_state() {
    let (mut parent, instance) = build_tiny_mem_store();

    write(&mut parent, instance, 0, 0x1111_1111);
    write(&mut parent, instance, 64, 0x2222_2222);

    let snap = parent.snapshot();
    let children = snap.fork(4).unwrap();

    for mut child in children {
        let inst = child.instance(0);
        assert_eq!(read(&mut child, inst, 0), 0x1111_1111);
        assert_eq!(read(&mut child, inst, 64), 0x2222_2222_u32 as i32);
    }
}

#[test]
fn fork_children_writes_are_isolated() {
    let (mut parent, instance) = build_tiny_mem_store();
    write(&mut parent, instance, 0, 0xAAAA_AAAA_u32 as i32);

    let snap = parent.snapshot();
    let mut children = snap.fork(3).unwrap();

    let inst0 = children[0].instance(0);
    let inst1 = children[1].instance(0);
    let inst2 = children[2].instance(0);

    write(&mut children[0], inst0, 0, 0xBBBB_BBBB_u32 as i32);
    write(&mut children[1], inst1, 0, 0xCCCC_CCCC_u32 as i32);

    assert_eq!(read(&mut children[0], inst0, 0), 0xBBBB_BBBB_u32 as i32);
    assert_eq!(read(&mut children[1], inst1, 0), 0xCCCC_CCCC_u32 as i32);
    assert_eq!(read(&mut children[2], inst2, 0), 0xAAAA_AAAA_u32 as i32);
}

#[test]
fn snapshot_can_be_forked_multiple_times() {
    let (mut parent, instance) = build_tiny_mem_store();
    write(&mut parent, instance, 100, 42);

    let snap = parent.snapshot();

    let first_batch = snap.fork(2).unwrap();
    let second_batch = snap.fork(3).unwrap();

    for mut child in first_batch.into_iter().chain(second_batch.into_iter()) {
        let inst = child.instance(0);
        assert_eq!(read(&mut child, inst, 100), 42);
    }
}

#[test]
fn fork_zero_returns_empty() {
    let (parent, _instance) = build_tiny_mem_store();
    let snap = parent.snapshot();
    let children = snap.fork(0).unwrap();
    assert!(children.is_empty());
}

#[test]
#[should_panic]
fn snapshot_panics_on_owned_store() {
    let wasm = wat::parse_str(TINY_MEM_WAT).unwrap();
    let module = Module::new(&wasm).unwrap();
    let mut store = Store::new();
    let _ = store.instantiate(&module, vec![]).unwrap();
    let _ = store.snapshot();
}

#[test]
fn forked_store_can_continue_executing() {
    let (mut parent, instance) = build_tiny_mem_store();
    write(&mut parent, instance, 0, 7);

    let snap = parent.snapshot();
    let mut children = snap.fork(2).unwrap();

    // each child does some independent work
    let inst0 = children[0].instance(0);
    let inst1 = children[1].instance(0);
    write(&mut children[0], inst0, 0, 100);
    write(&mut children[1], inst1, 0, 200);

    assert_eq!(read(&mut children[0], inst0, 0), 100);
    assert_eq!(read(&mut children[1], inst1, 0), 200);
}
