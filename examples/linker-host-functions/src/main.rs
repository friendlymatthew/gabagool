use std::error::Error as StdError;

use gabagool::{FunctionType, Linker, Module, RawValue, ResultType, Store, ValueType};

const WAT: &str = r#"
(module
  ;; i need the host to provide host.add_one
  (import "host" "add_one" (func $add_one (param i32) (result i32)))

  (func (export "run") (result i32)
    ;; pass 66 to rust, then return rust's result
    i32.const 66
    call $add_one)
)
"#;

fn main() -> std::result::Result<(), Box<dyn StdError>> {
    let wasm = wat::parse_str(WAT)?;
    let module = Module::new(&wasm)?;
    let mut store = Store::new();
    let mut linker = Linker::new();

    linker.func_new(
        "host",
        "add_one",
        func_type(vec![ValueType::I32], vec![ValueType::I32]),
        |_caller, args| Ok(vec![RawValue::from(args[0].as_i32() + 1)]),
    )?;

    let instance = linker.instantiate(&mut store, &module)?;
    let result = linker
        .invoke(&mut store, instance, "run", std::iter::empty::<RawValue>())?
        .into_completed()?;

    println!("wasm returned: {}", result[0].as_i32());

    Ok(())
}

fn func_type(params: Vec<ValueType>, results: Vec<ValueType>) -> FunctionType {
    FunctionType(ResultType(params), ResultType(results))
}
