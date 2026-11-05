use gabagool::{wasi, ExternalValue, Linker, Module, Store};

fn run(arguments: &[&str], argv: i32, buffer: i32) -> (i32, Vec<u8>) {
    run_list(arguments, argv, buffer, false)
}

fn run_list(strings: &[&str], argv: i32, buffer: i32, environment: bool) -> (i32, Vec<u8>) {
    let prefix = if environment { "environ" } else { "args" };
    let source = r#"(module
          (import "wasi_snapshot_preview1" "LIST_get"
            (func $args_get (param i32 i32) (result i32)))
          (import "wasi_snapshot_preview1" "LIST_sizes_get"
            (func $sizes (param i32 i32) (result i32)))
          (memory (export "memory") 1)
          (func (export "sizes") (result i32)
            i32.const 16 i32.const 20 call $sizes)
          (func (export "get") (param i32 i32) (result i32)
            local.get 0 local.get 1 call $args_get))"#
        .replace("LIST", prefix);
    let wasm = wat::parse_str(source).unwrap();
    let module = Module::try_new(&wasm).unwrap();
    let mut store = Store::new();
    let mut linker = Linker::new();
    wasi::add_wasip1_to_linker(&mut linker).unwrap();
    let instance = linker.instantiate(&mut store, &module).unwrap();
    let mut ctx: wasi::WasiCtx = wasi::WasiCtx::default();
    let list = if environment {
        ctx.environment_mut()
    } else {
        ctx.args_mut()
    };
    list.extend(strings.iter().map(|string| (*string).to_owned()));
    let sizes = linker
        .invoke_no_args(&mut store, &mut ctx, instance, "sizes")
        .unwrap()
        .into_completed()
        .unwrap();
    assert_eq!(sizes[0].as_i32(), 0);
    let result = linker
        .invoke(&mut store, &mut ctx, instance, "get", [argv, buffer])
        .unwrap()
        .into_completed()
        .unwrap()[0]
        .as_i32();
    let addr = store
        .exports(instance)
        .iter()
        .find_map(|export| {
            if export.name == "memory" {
                if let ExternalValue::Memory { addr } = export.value {
                    return Some(addr);
                }
            }
            None
        })
        .unwrap();
    (result, store.memories[addr].data.as_slice().to_vec())
}

fn u32_at(memory: &[u8], ptr: usize) -> u32 {
    u32::from_le_bytes(memory[ptr..ptr + 4].try_into().unwrap())
}

#[test]
fn copies_pointer_array_and_nul_terminated_strings() {
    let (errno, memory) = run(&["foo", "bar"], 0, 100);
    assert_eq!(errno, 0);
    assert_eq!(u32_at(&memory, 0), 100);
    assert_eq!(u32_at(&memory, 4), 104);
    assert_eq!(&memory[100..108], b"foo\0bar\0");
    assert_eq!(u32_at(&memory, 16), 2);
    assert_eq!(u32_at(&memory, 20), 8);
}

#[test]
fn handles_empty_and_multibyte_strings() {
    let (errno, memory) = run(&["", "é"], 0, 100);
    assert_eq!(errno, 0);
    assert_eq!(u32_at(&memory, 4), 101);
    assert_eq!(&memory[100..104], "\0é\0".as_bytes());
    assert_eq!(u32_at(&memory, 20), 4);
}

#[test]
fn handles_no_arguments() {
    let (errno, memory) = run(&[], 0, 100);
    assert_eq!(errno, 0);
    assert_eq!(u32_at(&memory, 16), 0);
    assert_eq!(u32_at(&memory, 20), 0);
}

#[test]
fn invalid_regions_return_fault_before_copying() {
    for (argv, buffer) in [(65532, 100), (0, 65532), (-1, 100), (0, -1)] {
        let (errno, memory) = run(&["foo", "bar"], argv, buffer);
        assert_eq!(errno, 21);
        assert_eq!(&memory[..8], &[0; 8]);
        assert_eq!(&memory[100..108], &[0; 8]);
    }
}

#[test]
fn environment_uses_its_own_entries() {
    let (errno, memory) = run_list(&["A=1", "B=2"], 0, 100, true);
    assert_eq!(errno, 0);
    assert_eq!(u32_at(&memory, 0), 100);
    assert_eq!(u32_at(&memory, 4), 104);
    assert_eq!(&memory[100..108], b"A=1\0B=2\0");
    assert_eq!(u32_at(&memory, 16), 2);
    assert_eq!(u32_at(&memory, 20), 8);
}

#[test]
fn empty_environment_and_invalid_regions() {
    let (errno, memory) = run_list(&[], 0, 100, true);
    assert_eq!(errno, 0);
    assert_eq!(u32_at(&memory, 16), 0);
    assert_eq!(u32_at(&memory, 20), 0);
    let (errno, _) = run_list(&["A=1"], 0, -1, true);
    assert_eq!(errno, 21);
}
