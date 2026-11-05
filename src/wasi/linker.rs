use crate::prelude::*;
use crate::{FunctionType, Linker, Result, ValueType};

use super::{WasiCtx, WasiOutput};

const WASI_MODULE_NAME: &str = "wasi_snapshot_preview1";

macro_rules! register_wasi {
    ($linker:expr, $method:ident, [$($param:ident),* $(,)?], [$($result:ident),* $(,)?] $(,)?) => {
        $linker.func_new(
            WASI_MODULE_NAME,
            stringify!($method),
            FunctionType {
                params: vec![$(ValueType::$param),*],
                results: vec![$(ValueType::$result),*],
            },
            |caller, args| Ok(super::methods::$method(caller, args)),
        )?;
    };
}

pub fn add_wasip1_to_linker<O: WasiOutput + 'static>(l: &mut Linker<WasiCtx<O>>) -> Result<()> {
    register_wasi!(l, proc_exit, [I32], []);
    register_wasi!(l, fd_write, [I32, I32, I32, I32], [I32]);
    register_wasi!(l, args_sizes_get, [I32, I32], [I32]);
    register_wasi!(l, args_get, [I32, I32], [I32]);

    Ok(())
}
