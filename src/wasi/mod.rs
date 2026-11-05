use crate::{prelude::*, ValueType};
use crate::{FunctionType, HostOutcome, Linker, Result};

pub fn add_to_linker<T>(l: &mut Linker<T>) -> Result<()> {
    // proc_exit
    l.func_new(
        "wasi_snapshot_preview1",
        "proc_exit",
        FunctionType {
            params: vec![ValueType::I32],
            results: vec![],
        },
        |_, args| {
            let code = args[0].as_i32();

            Ok(HostOutcome::Exit(code as u32))
        },
    )?;

    Ok(())
}
