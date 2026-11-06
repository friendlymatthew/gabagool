use crate::prelude::*;
use crate::{FunctionType, Linker, Result, ValueType};

use super::{WasiCtx, WasiResource};

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

pub fn add_wasip1_to_linker<R: WasiResource + 'static>(l: &mut Linker<WasiCtx<R>>) -> Result<()> {
    register_wasi!(l, proc_exit, [I32], []);
    register_wasi!(l, fd_write, [I32, I32, I32, I32], [I32]);
    register_wasi!(l, args_sizes_get, [I32, I32], [I32]);
    register_wasi!(l, args_get, [I32, I32], [I32]);
    register_wasi!(l, environ_sizes_get, [I32, I32], [I32]);
    register_wasi!(l, environ_get, [I32, I32], [I32]);
    register_wasi!(l, fd_fdstat_get, [I32, I32], [I32]);
    register_wasi!(l, fd_close, [I32], [I32]);
    register_wasi!(l, fd_read, [I32, I32, I32, I32], [I32]);
    register_wasi!(l, random_get, [I32, I32], [I32]);
    register_wasi!(l, clock_time_get, [I32, I64, I32], [I32]);
    register_wasi!(l, clock_res_get, [I32, I32], [I32]);
    register_wasi!(l, fd_fdstat_set_rights, [I32, I64, I64], [I32]);
    register_wasi!(l, fd_advise, [I32, I64, I64, I32], [I32]);
    register_wasi!(l, fd_datasync, [I32], [I32]);
    register_wasi!(l, fd_fdstat_set_flags, [I32, I32], [I32]);
    register_wasi!(l, fd_filestat_get, [I32, I32], [I32]);
    register_wasi!(l, fd_filestat_set_size, [I32, I64], [I32]);
    register_wasi!(l, fd_filestat_set_times, [I32, I64, I64, I32], [I32]);
    register_wasi!(l, fd_pread, [I32, I32, I32, I64, I32], [I32]);

    Ok(())
}
