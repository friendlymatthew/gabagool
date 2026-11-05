use crate::prelude::*;
use crate::wasi::{args_get, args_sizes_get, fd_write, WasiCtx, WasiOutput};
use crate::ValueType;
use crate::{FunctionType, HostOutcome, Linker, Result};

static WASI_MODULE_NAME: &str = "wasi_snapshot_preview1";

pub fn add_wasip1_to_linker<O: WasiOutput + 'static>(l: &mut Linker<WasiCtx<O>>) -> Result<()> {
    l.func_new(
        WASI_MODULE_NAME,
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

    l.func_new(
        WASI_MODULE_NAME,
        "fd_write",
        FunctionType {
            params: vec![ValueType::I32; 4],
            results: vec![ValueType::I32],
        },
        |caller, args| Ok(fd_write(caller, args)),
    )?;

    l.func_new(
        WASI_MODULE_NAME,
        "args_sizes_get",
        FunctionType {
            params: vec![ValueType::I32; 2],
            results: vec![ValueType::I32],
        },
        |caller, args| Ok(args_sizes_get(caller, args)),
    )?;

    l.func_new(
        WASI_MODULE_NAME,
        "args_get",
        FunctionType {
            params: vec![ValueType::I32; 2],
            results: vec![ValueType::I32],
        },
        |caller, args| Ok(args_get(caller, args)),
    )?;

    Ok(())
}
