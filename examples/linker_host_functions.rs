use std::error::Error as StdError;

use gabagool::{
    wasi, ExecutionState, FunctionType, HostOutcome, Linker, Module, RawValue, Store, ValueType,
};

const WAT: &str = r#"
(module
  ;; i need the host to provide host.add_one
  (import "host" "add_one" (func $add_one (param i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))

  (func (export "run") (result i32)
    ;; pass 66 to rust, then return rust's result
    i32.const 66
    call $add_one)

  (func (export "_start")
    i32.const 42
    call $proc_exit
    unreachable)
)
"#;

fn main() -> std::result::Result<(), Box<dyn StdError>> {
    let wasm = wat::parse_str(WAT)?;
    let module = Module::try_new(&wasm)?;
    let mut store = Store::new();
    let mut linker = Linker::new();

    linker.func_new(
        "host",
        "add_one",
        FunctionType {
            params: vec![ValueType::I32],
            results: vec![ValueType::I32],
        },
        |mut caller, args| {
            // an example of how you can update the caller's inner data from the host
            let d = caller.data_mut();
            *d += 1;

            Ok(HostOutcome::Return(vec![RawValue::from(
                args[0].as_i32() + 1,
            )]))
        },
    )?;

    wasi::add_to_linker(&mut linker)?;

    let instance = linker.instantiate(&mut store, &module)?;

    let mut data = 0;

    let result = linker
        .invoke_no_args(&mut store, &mut data, instance, "run")?
        .into_completed()?;

    assert_eq!(
        data, 1,
        "by now, data should have been incremented in the host callback fn"
    );
    assert_eq!(result[0].as_i32(), 67);

    println!("data: {data}");
    println!("result: {}", result[0].as_i32());

    let result = linker.invoke_no_args(&mut store, &mut data, instance, "_start")?;
    matches!(result, ExecutionState::Exited(42));

    Ok(())
}
